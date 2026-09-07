use super::*;

const CATALOG_PATH: &str = "conformance/0.5/racf/command-language.json";
const SCHEMA_PATH: &str = "conformance/0.5/schemas/racf-command-catalog.schema.json";
const CLASS_CATALOG_PATH: &str = "conformance/0.5/racf/supplied-classes.json";
const CLASS_SCHEMA_PATH: &str = "conformance/0.5/schemas/racf-class-catalog.schema.json";
const RACROUTE_PATH: &str = "conformance/0.5/racf/racroute.json";
const RACROUTE_SCHEMA_PATH: &str = "conformance/0.5/schemas/racroute-catalog.schema.json";
const ORACLE_SCHEMA_PATH: &str = "conformance/0.5/schemas/racf-oracle-campaign.schema.json";
const DISPOSITIONS_PATH: &str = "conformance/0.5/racf/operand-dispositions.json";
const DISPOSITIONS_SCHEMA_PATH: &str =
    "conformance/0.5/schemas/racf-operand-dispositions.schema.json";
const PROJECTION_PATH: &str = "conformance/0.5/generated/racf-html-syntax-projection.json";
const GENERATED_PATH: &str =
    "crates/providers/mainframe-env-racf/src/generated/racf_command_catalog.rs";
const SPEC_PATH: &str = "conformance/spec/v1/spec.json";
const RACF_ROW_PREFIX: &str = "ibm-zos-3.2-racf-saf-2026:racf-command-families:";
const RACROUTE_ROW_PREFIX: &str = "ibm-zos-3.2-racf-saf-2026:racroute-request-types:";
const RACF_ORACLE_ID: &str = "racf.zos32.licensed-campaign";

pub(super) fn generate(root: &Path) -> TaskResult {
    let generated = render(root)?;
    let path = root.join(GENERATED_PATH);
    fs::create_dir_all(path.parent().ok_or("generated RACF path has no parent")?)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    fs::write(&path, generated).map_err(|error| format!("{}: {error}", path.display()))?;
    let spec_path = root.join(SPEC_PATH);
    fs::write(&spec_path, project_spec(root)?)
        .map_err(|error| format!("{}: {error}", spec_path.display()))
}

pub(super) fn check(root: &Path) -> TaskResult {
    let expected = render(root)?;
    let path = root.join(GENERATED_PATH);
    let actual = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    require(
        actual == expected,
        "generated RACF command catalog is stale; run cargo xtask racf-catalog",
    )?;
    let spec_path = root.join(SPEC_PATH);
    let actual_spec =
        fs::read(&spec_path).map_err(|error| format!("{}: {error}", spec_path.display()))?;
    require(
        actual_spec == project_spec(root)?,
        "shared Conformance IR RACF projection is stale; run cargo xtask racf-catalog",
    )
}

fn project_spec(root: &Path) -> TaskResult<Vec<u8>> {
    let catalog_path = root.join(CATALOG_PATH);
    let catalog = json(&catalog_path)?;
    let families = array(&catalog, "families", &catalog_path)?;
    let spec_path = root.join(SPEC_PATH);
    let mut spec = json(&spec_path)?;
    let oracle_path = root.join(RACF_ORACLE_RELATIVE_PATH);
    if oracle_path.is_file() {
        validate_schema_instance(
            &json(&root.join(ORACLE_SCHEMA_PATH))?,
            &json(&oracle_path)?,
            &oracle_path,
        )?;
    }
    let oracle = RacfOracleCampaign::load_optional(root)?;
    let oracle_id = oracle.as_ref().map(|_| RACF_ORACLE_ID);
    {
        let registries = spec["registries"]
            .as_object_mut()
            .ok_or("shared Conformance IR registries are not an object")?;
        for (name, values) in [
            ("operations", vec!["racf.command", "racf.racroute"]),
            (
                "input_shapes",
                vec!["racf.command.text", "racf.racroute.typed"],
            ),
            ("predicates", vec!["racf.authority.ready"]),
            (
                "transitions",
                vec!["racf.command.transition", "racf.racroute.transition"],
            ),
            ("observations", vec!["racf.command.passed"]),
            (
                "conditions",
                vec!["racf.command.diagnostic", "racf.racroute.status"],
            ),
            ("recoveries", vec!["racf.restart-recovery"]),
            (
                "drivers",
                vec!["racf.command.driver", "racf.racroute.driver"],
            ),
            ("scenario_steps", Vec::new()),
            ("failure_points", Vec::new()),
        ] {
            replace_string_registry(registries, name, &values)?;
        }
        let oracle_entries = oracle.as_ref().map_or_else(Vec::new, |campaign| {
            vec![(RACF_ORACLE_ID.into(), campaign.digest().into())]
        });
        replace_artifact_registry(registries, "oracles", &oracle_entries)?;
    }

    let mut rows = spec["rows"]
        .as_array()
        .cloned()
        .ok_or("shared Conformance IR rows are not an array")?;
    rows.retain(|row| {
        !row["row_id"].as_str().is_some_and(|row_id| {
            row_id.starts_with(RACF_ROW_PREFIX) || row_id.starts_with(RACROUTE_ROW_PREFIX)
        })
    });
    let mut obligations = spec["obligations"]
        .as_array()
        .cloned()
        .ok_or("shared Conformance IR obligations are not an array")?;
    obligations.retain(|obligation| {
        !obligation["row_id"].as_str().is_some_and(|row_id| {
            row_id.starts_with(RACF_ROW_PREFIX) || row_id.starts_with(RACROUTE_ROW_PREFIX)
        })
    });
    let mut cases = spec["cases"]
        .as_array()
        .cloned()
        .ok_or("shared Conformance IR cases are not an array")?;
    cases.retain(|case| {
        !case["row_id"].as_str().is_some_and(|row_id| {
            row_id.starts_with(RACF_ROW_PREFIX) || row_id.starts_with(RACROUTE_ROW_PREFIX)
        })
    });

    let mut fixtures = Vec::new();
    for (index, family) in families.iter().enumerate() {
        let row_id = text(family, "row_id", &catalog_path)?;
        let sequence = index + 1;
        let executable = matches!(
            text(family, "work_package", &catalog_path)?,
            "SEC-502" | "SEC-503" | "SEC-505"
        );
        let mut obligation_ids = vec![
            "syntax",
            "malformed",
            "bounded-limit",
            "audit-redaction",
            "atomic-retry",
            "restart-recovery",
        ];
        if executable {
            obligation_ids.extend(["authorized", "unauthorized"]);
        }
        if oracle.is_some() {
            obligation_ids.push("licensed-equivalence");
        }
        rows.push(json!({
            "row_id": row_id,
            "operation": "racf.command",
            "input": "racf.command.text",
            "preconditions": ["racf.authority.ready"],
            "transition": "racf.command.transition",
            "postconditions": ["racf.command.passed"],
            "conditions": ["racf.command.diagnostic"],
            "recovery": "racf.restart-recovery",
            "oracle": oracle_id,
            "applicable_gates": ["recognized", "validated", "executed", "conditioned", "recovered", "differential"],
            "obligations": obligation_ids
        }));
        for (obligation, gates) in [
            ("syntax", vec!["recognized", "validated"]),
            ("malformed", vec!["conditioned"]),
        ] {
            obligations.push(json!({
                "row_id": row_id,
                "obligation_id": obligation,
                "applicable_gates": gates,
            }));
            for gate in gates {
                push_case(
                    &mut cases,
                    &mut fixtures,
                    "command",
                    row_id,
                    sequence,
                    obligation,
                    gate,
                    oracle_id,
                );
            }
        }
        if executable {
            obligations.push(json!({
                "row_id": row_id,
                "obligation_id": "authorized",
                "applicable_gates": ["executed"],
            }));
            push_case(
                &mut cases,
                &mut fixtures,
                "command",
                row_id,
                sequence,
                "authorized",
                "executed",
                oracle_id,
            );
            obligations.push(json!({
                "row_id": row_id,
                "obligation_id": "unauthorized",
                "applicable_gates": ["executed", "conditioned"],
            }));
            for gate in ["executed", "conditioned"] {
                push_case(
                    &mut cases,
                    &mut fixtures,
                    "command",
                    row_id,
                    sequence,
                    "unauthorized",
                    gate,
                    oracle_id,
                );
            }
        }
        obligations.push(json!({
            "row_id": row_id,
            "obligation_id": "restart-recovery",
            "applicable_gates": ["recovered"],
        }));
        push_case(
            &mut cases,
            &mut fixtures,
            "command",
            row_id,
            sequence,
            "restart-recovery",
            "recovered",
            oracle_id,
        );
        for (obligation, gate) in [
            ("bounded-limit", "conditioned"),
            ("audit-redaction", "conditioned"),
        ] {
            obligations.push(json!({
                "row_id": row_id,
                "obligation_id": obligation,
                "applicable_gates": [gate],
            }));
            push_case(
                &mut cases,
                &mut fixtures,
                "command",
                row_id,
                sequence,
                obligation,
                gate,
                oracle_id,
            );
        }
        obligations.push(json!({
            "row_id": row_id,
            "obligation_id": "atomic-retry",
            "applicable_gates": ["recovered"],
        }));
        push_case(
            &mut cases,
            &mut fixtures,
            "command",
            row_id,
            sequence,
            "atomic-retry",
            "recovered",
            oracle_id,
        );
        if let Some(campaign) = &oracle {
            require_oracle_fixture(campaign, row_id, "command", sequence)?;
            obligations.push(json!({
                "row_id": row_id,
                "obligation_id": "licensed-equivalence",
                "applicable_gates": ["differential"],
            }));
            push_case(
                &mut cases,
                &mut fixtures,
                "command",
                row_id,
                sequence,
                "licensed-equivalence",
                "differential",
                oracle_id,
            );
        }
    }
    let racroute_path = root.join(RACROUTE_PATH);
    let racroute = json(&racroute_path)?;
    for (index, request) in array(&racroute, "requests", &racroute_path)?
        .iter()
        .enumerate()
    {
        let row_id = text(request, "row_id", &racroute_path)?;
        let sequence = index + 1;
        rows.push(json!({
            "row_id": row_id,
            "operation": "racf.racroute",
            "input": "racf.racroute.typed",
            "preconditions": ["racf.authority.ready"],
            "transition": "racf.racroute.transition",
            "postconditions": ["racf.command.passed"],
            "conditions": ["racf.racroute.status"],
            "recovery": "racf.restart-recovery",
            "oracle": oracle_id,
            "applicable_gates": ["recognized", "validated", "executed", "conditioned", "recovered", "differential"],
            "obligations": if oracle.is_some() {
                json!(["syntax", "authorized", "unauthorized", "malformed", "bounded-limit", "audit-redaction", "atomic-retry", "restart-recovery", "licensed-equivalence"])
            } else {
                json!(["syntax", "authorized", "unauthorized", "malformed", "bounded-limit", "audit-redaction", "atomic-retry", "restart-recovery"])
            }
        }));
        for (obligation, gates) in [
            ("syntax", vec!["recognized", "validated"]),
            ("authorized", vec!["executed"]),
            ("unauthorized", vec!["executed", "conditioned"]),
            ("malformed", vec!["conditioned"]),
        ] {
            obligations.push(json!({
                "row_id": row_id,
                "obligation_id": obligation,
                "applicable_gates": gates,
            }));
            for gate in gates {
                push_case(
                    &mut cases,
                    &mut fixtures,
                    "racroute",
                    row_id,
                    sequence,
                    obligation,
                    gate,
                    oracle_id,
                );
            }
        }
        obligations.push(json!({
            "row_id": row_id,
            "obligation_id": "restart-recovery",
            "applicable_gates": ["recovered"],
        }));
        push_case(
            &mut cases,
            &mut fixtures,
            "racroute",
            row_id,
            sequence,
            "restart-recovery",
            "recovered",
            oracle_id,
        );
        for (obligation, gate) in [
            ("bounded-limit", "conditioned"),
            ("audit-redaction", "conditioned"),
        ] {
            obligations.push(json!({
                "row_id": row_id,
                "obligation_id": obligation,
                "applicable_gates": [gate],
            }));
            push_case(
                &mut cases,
                &mut fixtures,
                "racroute",
                row_id,
                sequence,
                obligation,
                gate,
                oracle_id,
            );
        }
        obligations.push(json!({
            "row_id": row_id,
            "obligation_id": "atomic-retry",
            "applicable_gates": ["recovered"],
        }));
        push_case(
            &mut cases,
            &mut fixtures,
            "racroute",
            row_id,
            sequence,
            "atomic-retry",
            "recovered",
            oracle_id,
        );
        if let Some(campaign) = &oracle {
            require_oracle_fixture(campaign, row_id, "racroute", sequence)?;
            obligations.push(json!({
                "row_id": row_id,
                "obligation_id": "licensed-equivalence",
                "applicable_gates": ["differential"],
            }));
            push_case(
                &mut cases,
                &mut fixtures,
                "racroute",
                row_id,
                sequence,
                "licensed-equivalence",
                "differential",
                oracle_id,
            );
        }
    }
    rows.sort_by(|left, right| left["row_id"].as_str().cmp(&right["row_id"].as_str()));
    obligations.sort_by(|left, right| {
        (left["row_id"].as_str(), left["obligation_id"].as_str())
            .cmp(&(right["row_id"].as_str(), right["obligation_id"].as_str()))
    });
    cases.sort_by(|left, right| {
        (
            left["row_id"].as_str(),
            left["obligation_id"].as_str(),
            left["gate"].as_str(),
        )
            .cmp(&(
                right["row_id"].as_str(),
                right["obligation_id"].as_str(),
                right["gate"].as_str(),
            ))
    });
    spec["rows"] = Value::Array(rows);
    spec["obligations"] = Value::Array(obligations);
    spec["cases"] = Value::Array(cases);
    let registries = spec["registries"]
        .as_object_mut()
        .ok_or("shared Conformance IR registries are not an object")?;
    replace_artifact_registry(registries, "fixtures", &fixtures)?;
    let mut bytes = serde_json::to_vec_pretty(&spec).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn push_case(
    cases: &mut Vec<Value>,
    fixtures: &mut Vec<(String, String)>,
    surface: &str,
    row_id: &str,
    sequence: usize,
    obligation: &str,
    gate: &str,
    oracle: Option<&str>,
) {
    let (test_id, fixture, digest) = case_identity(surface, sequence, obligation, gate);
    fixtures.push((fixture.clone(), digest));
    cases.push(json!({
        "spec_version": "mainframe-env.conformance-ir@1",
        "row_id": row_id,
        "obligation_id": obligation,
        "gate": gate,
        "test_id": test_id,
        "driver": format!("racf.{surface}.driver"),
        "input": fixture,
        "preconditions": ["racf.authority.ready"],
        "expected": ["racf.command.passed"],
        "recovery": "racf.restart-recovery",
        "oracle": oracle,
    }));
}

fn case_identity(
    surface: &str,
    sequence: usize,
    obligation: &str,
    gate: &str,
) -> (String, String, String) {
    let test_id = format!("racf.{surface}.{sequence:04}.{obligation}.{gate}");
    let fixture = format!("{test_id}.fixture");
    let digest = format!("sha256:{:x}", Sha256::digest(fixture.as_bytes()));
    (test_id, fixture, digest)
}

fn require_oracle_fixture(
    campaign: &RacfOracleCampaign,
    row_id: &str,
    surface: &str,
    sequence: usize,
) -> TaskResult {
    let oracle_case = campaign
        .case(row_id)
        .ok_or_else(|| format!("licensed RACF oracle row is missing: {row_id}"))?;
    let (_, _, expected) = case_identity(surface, sequence, "licensed-equivalence", "differential");
    require(
        oracle_case.fixture_digest == expected,
        &format!("licensed RACF oracle fixture drifted for {row_id}"),
    )
}

fn replace_string_registry(
    registries: &mut serde_json::Map<String, Value>,
    name: &str,
    additions: &[&str],
) -> TaskResult {
    let values = registries
        .get_mut(name)
        .and_then(Value::as_array_mut)
        .ok_or_else(|| format!("shared Conformance IR registry {name} is not an array"))?;
    values.retain(|value| {
        !value
            .as_str()
            .is_some_and(|value| value.starts_with("racf."))
    });
    values.extend(additions.iter().map(|value| Value::String((*value).into())));
    values.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    Ok(())
}

fn replace_artifact_registry(
    registries: &mut serde_json::Map<String, Value>,
    name: &str,
    additions: &[(String, String)],
) -> TaskResult {
    let values = registries
        .get_mut(name)
        .and_then(Value::as_array_mut)
        .ok_or_else(|| format!("shared Conformance IR registry {name} is not an array"))?;
    values.retain(|value| {
        !value["id"]
            .as_str()
            .is_some_and(|value| value.starts_with("racf."))
    });
    values.extend(
        additions
            .iter()
            .map(|(id, digest)| json!({"id": id, "digest": digest})),
    );
    values.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    Ok(())
}

fn render(root: &Path) -> TaskResult<Vec<u8>> {
    let catalog_path = root.join(CATALOG_PATH);
    let catalog = json(&catalog_path)?;
    validate_schema_instance(&json(&root.join(SCHEMA_PATH))?, &catalog, &catalog_path)?;
    let families = array(&catalog, "families", &catalog_path)?;
    require(
        families.len() == 34,
        "RACF command catalog must contain 34 families",
    )?;
    let official_path = root.join("conformance/0.2/catalogs/racf-saf.json");
    let official = json(&official_path)?;
    let units = array(&official, "units", &official_path)?;
    let official_rows = array(&units[0], "rows", &official_path)?;
    require(
        official_rows.len() == families.len(),
        "RACF generated/official command denominators differ",
    )?;
    let class_path = root.join(CLASS_CATALOG_PATH);
    let class_catalog = json(&class_path)?;
    validate_schema_instance(
        &json(&root.join(CLASS_SCHEMA_PATH))?,
        &class_catalog,
        &class_path,
    )?;
    let classes = array(&class_catalog, "classes", &class_path)?;
    let mut class_names = BTreeSet::new();
    for class in classes {
        let name = text(class, "name", &class_path)?;
        require(
            class_names.insert(name),
            &format!("duplicate RACF supplied class {name}"),
        )?;
    }
    let racroute_path = root.join(RACROUTE_PATH);
    let racroute = json(&racroute_path)?;
    validate_schema_instance(
        &json(&root.join(RACROUTE_SCHEMA_PATH))?,
        &racroute,
        &racroute_path,
    )?;
    let requests = array(&racroute, "requests", &racroute_path)?;
    let official_requests = array(&units[1], "rows", &official_path)?;
    require(
        requests.len() == 14 && official_requests.len() == requests.len(),
        "RACROUTE generated/official request denominators differ",
    )?;
    let mut request_variants = BTreeSet::new();
    for (index, request) in requests.iter().enumerate() {
        let variant = text(request, "variant", &racroute_path)?;
        require(
            request_variants.insert(variant),
            &format!("duplicate RACROUTE request variant {variant}"),
        )?;
        require(
            text(request, "row_id", &racroute_path)?
                == text(&official_requests[index], "id", &official_path)?
                && text(request, "keyword", &racroute_path)?
                    == text(&official_requests[index], "label", &official_path)?,
            &format!("RACROUTE request identity drifted at {variant}"),
        )?;
    }

    let mut variants = BTreeSet::new();
    let mut selectors = BTreeSet::new();
    for (index, family) in families.iter().enumerate() {
        let variant = text(family, "variant", &catalog_path)?;
        let keyword = text(family, "keyword", &catalog_path)?;
        require(
            variants.insert(variant),
            &format!("duplicate RACF command variant {variant}"),
        )?;
        require(
            selectors.insert(keyword),
            &format!("duplicate RACF command keyword {keyword}"),
        )?;
        for alias in string_array(family, "aliases", &catalog_path)? {
            require(
                selectors.insert(alias),
                &format!("duplicate RACF command selector {alias}"),
            )?;
        }
        let row_id = text(family, "row_id", &catalog_path)?;
        require(
            text(&official_rows[index], "id", &official_path)? == row_id,
            &format!("RACF command row order/identity drifted at {row_id}"),
        )?;
        let official_keyword = text(&official_rows[index], "label", &official_path)?
            .split_whitespace()
            .next()
            .ok_or("official RACF label is empty")?;
        require(
            official_keyword == keyword,
            &format!("RACF command keyword differs from official row {row_id}"),
        )?;
        let min = integer(family, "min_positionals", &catalog_path)?;
        let max = integer(family, "max_positionals", &catalog_path)?;
        require(
            min <= max,
            &format!("RACF positional bounds invert for {keyword}"),
        )?;
        let operands = string_array(family, "operands", &catalog_path)?;
        let unique = operands.iter().copied().collect::<BTreeSet<_>>();
        require(
            unique.len() == operands.len(),
            &format!("duplicate RACF operand for {keyword}"),
        )?;
    }

    check_operand_dispositions(root, families, &catalog_path)?;

    let mut out = String::from(
        "// @generated by `cargo xtask racf-catalog`; do not edit.\n\n\
         #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]\n\
         pub enum CommandFamily {\n",
    );
    for family in families {
        out.push_str("    ");
        out.push_str(text(family, "variant", &catalog_path)?);
        out.push_str(",\n");
    }
    out.push_str("}\n\npub const COMMAND_DESCRIPTORS: &[CommandDescriptor] = &[\n");
    for family in families {
        out.push_str("    CommandDescriptor {\n");
        out.push_str(&format!(
            "        family: CommandFamily::{},\n        row_id: {:?},\n        keyword: {:?},\n",
            text(family, "variant", &catalog_path)?,
            text(family, "row_id", &catalog_path)?,
            text(family, "keyword", &catalog_path)?,
        ));
        out.push_str("        aliases: &[");
        separated_strings(&mut out, string_array(family, "aliases", &catalog_path)?);
        out.push_str("],\n");
        out.push_str(&format!(
            "        domain: CommandDomain::{},\n        work_package: {:?},\n        mutating: {},\n        command_direction: {},\n        min_positionals: {},\n        max_positionals: {},\n",
            rust_domain(text(family, "domain", &catalog_path)?)?,
            text(family, "work_package", &catalog_path)?,
            boolean(family, "mutating", &catalog_path)?,
            boolean(family, "command_direction", &catalog_path)?,
            integer(family, "min_positionals", &catalog_path)?,
            integer(family, "max_positionals", &catalog_path)?,
        ));
        out.push_str("        operands: &[");
        separated_strings(&mut out, string_array(family, "operands", &catalog_path)?);
        out.push_str("],\n    },\n");
    }
    out.push_str("];\n\npub const SUPPLIED_CLASS_DESCRIPTORS: &[SuppliedClassDescriptor] = &[\n");
    for class in classes {
        let posit = class["posit"]
            .as_u64()
            .map_or_else(|| "None".to_string(), |value| format!("Some({value})"));
        out.push_str(&format!(
            "    SuppliedClassDescriptor {{ name: {:?}, active: {}, generic_allowed: {}, generic_active: {}, discrete_allowed: {}, raclist: {}, max_profile_name_bytes: {}, posit: {} }},\n",
            text(class, "name", &class_path)?,
            boolean(class, "active", &class_path)?,
            boolean(class, "generic_allowed", &class_path)?,
            boolean(class, "generic_active", &class_path)?,
            boolean(class, "discrete_allowed", &class_path)?,
            boolean(class, "raclist", &class_path)?,
            integer(class, "max_profile_name_bytes", &class_path)?,
            posit,
        ));
    }
    out.push_str("];\n");
    out.push_str(
        "\n#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]\n\
         pub enum RacrouteRequestType {\n",
    );
    for request in requests {
        out.push_str("    ");
        out.push_str(text(request, "variant", &racroute_path)?);
        out.push_str(",\n");
    }
    out.push_str("}\n\npub const RACROUTE_DESCRIPTORS: &[RacrouteDescriptor] = &[\n");
    for request in requests {
        out.push_str(&format!(
            "    RacrouteDescriptor {{ request_type: RacrouteRequestType::{}, row_id: {:?}, keyword: {:?}, mutating: {}, requires_acee: {}, uses_cache: {} }},\n",
            text(request, "variant", &racroute_path)?,
            text(request, "row_id", &racroute_path)?,
            text(request, "keyword", &racroute_path)?,
            boolean(request, "mutating", &racroute_path)?,
            boolean(request, "requires_acee", &racroute_path)?,
            boolean(request, "uses_cache", &racroute_path)?,
        ));
    }
    out.push_str("];\n");
    Ok(out.into_bytes())
}

/// Hold the catalog to the publication for every operand name the publication does not publish.
///
/// The RACF reader reports, per family, the operand names this catalog carries that the pinned
/// topic does not publish as a top-level operand of that command (`catalog_only`). Those names are
/// normative row content and nothing else in the repository judges them, so the rule enforced here
/// is that every one of them is accounted for by name in
/// `conformance/0.5/racf/operand-dispositions.json`: a name is either applied -- rewritten to the
/// operand the publication does publish -- or explained, with a reason a reviewer can act on.
///
/// The load-bearing clause is the set equality. The dispositions that are not applied must be
/// exactly the `catalog_only` names the projection reports today, so adding an unpublished operand
/// to the catalog fails this gate until it is dispositioned, and dispositioning a name that is no
/// longer catalog-only fails it as well. An applied disposition is checked from the other side:
/// the name it replaced must be gone from the family's operands and the published name must be
/// there. The projection is regenerated from the pinned topics by
/// `conformance/0.5/tools/extract_racf_html_syntax.py`, so this also fails if the projection and
/// the catalog have drifted apart.
fn check_operand_dispositions(root: &Path, families: &[Value], catalog_path: &Path) -> TaskResult {
    let dispositions_path = root.join(DISPOSITIONS_PATH);
    let dispositions = json(&dispositions_path)?;
    validate_schema_instance(
        &json(&root.join(DISPOSITIONS_SCHEMA_PATH))?,
        &dispositions,
        &dispositions_path,
    )?;

    let mut catalog_operands = BTreeMap::new();
    for family in families {
        catalog_operands.insert(
            text(family, "row_id", catalog_path)?,
            (
                text(family, "keyword", catalog_path)?,
                string_array(family, "operands", catalog_path)?
                    .into_iter()
                    .collect::<BTreeSet<_>>(),
            ),
        );
    }

    // What the publication does not publish, as the reader last measured it.
    let projection_path = root.join(PROJECTION_PATH);
    let projection = json(&projection_path)?;
    let mut catalog_only = BTreeSet::new();
    let rows = array(&projection, "rows", &projection_path)?;
    require(
        rows.len() == catalog_operands.len(),
        "RACF syntax projection and command catalog disagree on the family denominator",
    )?;
    for row in rows {
        let row_id = text(row, "row_id", &projection_path)?;
        let (keyword, operands) = catalog_operands
            .get(row_id)
            .ok_or_else(|| format!("RACF syntax projection names unknown row {row_id}"))?;
        // Without this the gate would read `catalog_only` from a projection measured against some
        // earlier catalog, and an operand added since would be neither reported nor dispositioned.
        require(
            string_array(row, "catalog_operands", &projection_path)?
                .into_iter()
                .collect::<BTreeSet<_>>()
                == *operands,
            &format!(
                "RACF syntax projection is stale for {keyword}; regenerate it with conformance/0.5/tools/extract_racf_html_syntax.py"
            ),
        )?;
        for name in string_array(row, "catalog_only", &projection_path)? {
            catalog_only.insert((row_id, name));
        }
    }

    let entries = array(&dispositions, "dispositions", &dispositions_path)?;
    let mut seen = BTreeSet::new();
    let mut applied = BTreeSet::new();
    let mut deferred = BTreeSet::new();
    for entry in entries {
        let row_id = text(entry, "row_id", &dispositions_path)?;
        let operand = text(entry, "operand_name", &dispositions_path)?;
        let (keyword, operands) = catalog_operands.get(row_id).ok_or_else(|| {
            format!(
                "{} disposes unknown row {row_id}",
                dispositions_path.display()
            )
        })?;
        require(
            text(entry, "keyword", &dispositions_path)? == *keyword,
            &format!("RACF operand disposition keyword differs from the catalog at {row_id}"),
        )?;
        require(
            seen.insert((row_id, operand)),
            &format!("duplicate RACF operand disposition for {keyword} {operand}"),
        )?;
        if boolean(entry, "applied", &dispositions_path)? {
            let published = text(entry, "published_operand", &dispositions_path)?;
            require(
                !operands.contains(operand),
                &format!("{keyword} still carries the replaced operand {operand}"),
            )?;
            require(
                operands.contains(published),
                &format!("{keyword} does not carry the published operand {published}"),
            )?;
            applied.insert((row_id, operand));
        } else {
            require(
                operands.contains(operand),
                &format!("{keyword} no longer carries the deferred operand {operand}"),
            )?;
            deferred.insert((row_id, operand));
        }
    }

    for (row_id, name) in &catalog_only {
        require(
            deferred.contains(&(*row_id, *name)),
            &format!(
                "RACF operand {name} of row {row_id} is not published and is not dispositioned"
            ),
        )?;
    }
    for (row_id, name) in &deferred {
        require(
            catalog_only.contains(&(*row_id, *name)),
            &format!(
                "RACF operand disposition for {name} of row {row_id} is stale; the projection reports it as published"
            ),
        )?;
    }

    let families_of = |set: &BTreeSet<(&str, &str)>| {
        set.iter()
            .map(|(row_id, _)| *row_id)
            .collect::<BTreeSet<_>>()
            .len() as u64
    };
    let baseline = applied.union(&deferred).copied().collect::<BTreeSet<_>>();
    for (field, expected) in [
        ("baseline_catalog_only_names", baseline.len() as u64),
        ("baseline_catalog_only_families", families_of(&baseline)),
        ("applied_names", applied.len() as u64),
        ("remaining_catalog_only_names", deferred.len() as u64),
        ("remaining_catalog_only_families", families_of(&deferred)),
    ] {
        require(
            integer(&dispositions, field, &dispositions_path)? == expected,
            &format!(
                "{} {field} is {}, but the dispositions count {expected}",
                dispositions_path.display(),
                integer(&dispositions, field, &dispositions_path)?
            ),
        )?;
    }
    Ok(())
}

fn string_array<'a>(value: &'a Value, field: &str, path: &Path) -> TaskResult<Vec<&'a str>> {
    array(value, field, path)?
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .ok_or_else(|| format!("{} {field} contains a non-string", path.display()))
        })
        .collect()
}

fn integer(value: &Value, field: &str, path: &Path) -> TaskResult<u64> {
    value[field]
        .as_u64()
        .ok_or_else(|| format!("{} {field} is not an unsigned integer", path.display()))
}

fn boolean(value: &Value, field: &str, path: &Path) -> TaskResult<bool> {
    value[field]
        .as_bool()
        .ok_or_else(|| format!("{} {field} is not a boolean", path.display()))
}

fn separated_strings(out: &mut String, values: Vec<&str>) {
    for (index, value) in values.into_iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!("{value:?}"));
    }
}

fn rust_domain(value: &str) -> TaskResult<&'static str> {
    match value {
        "group" => Ok("Group"),
        "dataset" => Ok("Dataset"),
        "user" => Ok("User"),
        "connection" => Ok("Connection"),
        "resource" => Ok("Resource"),
        "query" => Ok("Query"),
        "policy" => Ok("Policy"),
        "identity" => Ok("Identity"),
        "operations" => Ok("Operations"),
        _ => Err(format!("unknown RACF command domain {value}")),
    }
}
