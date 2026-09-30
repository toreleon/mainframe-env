use super::*;

const SOURCE_PATH: &str = "conformance/0.2/catalogs/ims.json";
const MANIFEST_PATH: &str = "conformance/0.2/manifests/ims-topics.json";
const GENERATED_PATH: &str =
    "crates/foundation/mainframe-env-ir/src/generated/ims_call_registry.rs";
const SSA_RULES_PATH: &str = "conformance/0.14/ims/ssa-rules.json";
const SSA_SCHEMA_PATH: &str = "conformance/0.14/schemas/ims-ssa-rules.schema.json";
const SSA_MANIFEST_PATH: &str = "conformance/0.14/manifests/ims-programming-contracts-topics.json";
const GENERATED_SSA_PATH: &str =
    "crates/contracts/mainframe-env-host-api/src/generated/ims_ssa_rules.rs";
const BASELINE: &str = "ibm-ims-15.6-dli-2026-08-31";
const SOURCE_TOPIC: &str =
    "SSEPH2_15.6.0/com.ibm.ims156.doc.apg/ims_comparingexecdlicmdsanddlicalls.htm";
const REGISTRY_DOMAIN: &[u8] = b"mainframe-env.ims-call-registry@1\0";

#[derive(Clone, Debug)]
struct Family {
    ordinal: u8,
    official_row: String,
    label: String,
    source_locator: String,
    call_names: Vec<String>,
    command_names: Vec<String>,
}

pub(super) fn generate(root: &Path) -> TaskResult {
    for (relative, generated) in [
        (GENERATED_PATH, render(root)?),
        (GENERATED_SSA_PATH, render_ssa(root)?),
    ] {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().ok_or("generated IMS path has no parent")?)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        fs::write(&path, generated).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(())
}

pub(super) fn check(root: &Path) -> TaskResult {
    for (relative, expected) in [
        (GENERATED_PATH, render(root)?),
        (GENERATED_SSA_PATH, render_ssa(root)?),
    ] {
        let path = root.join(relative);
        let actual = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        require(
            actual == expected,
            &format!("{relative} is stale; run cargo xtask ims-catalog"),
        )?;
    }
    Ok(())
}

fn render(root: &Path) -> TaskResult<Vec<u8>> {
    let source_path = root.join(SOURCE_PATH);
    let source = json(&source_path)?;
    require(
        source["schema_version"] == Value::String("mainframe-env.official-catalog@1".into())
            && source["baseline_id"] == Value::String(BASELINE.into())
            && source["subsystem"] == Value::String("ims".into())
            && source["mandatory_rows"].as_u64() == Some(25),
        "pinned IMS catalog identity or denominator drifted",
    )?;
    let units = array(&source, "units", &source_path)?;
    require(units.len() == 1, "pinned IMS catalog must contain one unit")?;
    let unit = &units[0];
    require(
        text(unit, "id", &source_path)? == "dli-call-families"
            && unit["denominator"].as_u64() == Some(25)
            && unit["normalization"] == Value::String("normalized".into()),
        "pinned IMS call-family unit identity drifted",
    )?;
    let rows = array(unit, "rows", &source_path)?;
    require(rows.len() == 25, "pinned IMS catalog must contain 25 rows")?;

    let manifest_path = root.join(MANIFEST_PATH);
    let manifest = json(&manifest_path)?;
    require(
        manifest["schema_version"] == Value::String("mainframe-env.topic-manifest@1".into())
            && manifest["baseline_id"] == Value::String(BASELINE.into())
            && manifest["subsystem"] == Value::String("ims".into())
            && manifest["topic_count"].as_u64() == Some(1),
        "pinned IMS topic manifest identity drifted",
    )?;
    let topics = array(&manifest, "topics", &manifest_path)?;
    let topic = &topics[0];
    require(
        text(topic, "topic_path", &manifest_path)? == SOURCE_TOPIC
            && text(topic, "sha256", &manifest_path)?
                == "ce4a179eac0f18ed4ff71bed9ca576b31bcbbdb076d305dbcbf942e26ce13e30"
            && topic["bytes"].as_u64() == Some(16_046),
        "pinned IMS comparison topic identity drifted",
    )?;

    let mut families = Vec::with_capacity(rows.len());
    let mut seen_rows = BTreeSet::new();
    for (index, row) in rows.iter().enumerate() {
        let ordinal = u8::try_from(index + 1).map_err(|error| error.to_string())?;
        let official_row = text(row, "id", &source_path)?.to_string();
        let label = text(row, "label", &source_path)?.to_string();
        let source_locator = text(row, "source_locator", &source_path)?.to_string();
        require(
            row["mandatory"] == Value::Bool(true)
                && official_row == format!("{BASELINE}:dli-call-families:{ordinal:04}")
                && seen_rows.insert(official_row.clone()),
            &format!("IMS row identity is invalid or duplicated: {official_row}"),
        )?;
        let expected_prefix = format!("html-table:comparison;row:{ordinal};command:");
        let command_label = source_locator
            .strip_prefix(&expected_prefix)
            .ok_or_else(|| {
                format!("IMS row {official_row} has an invalid comparison-table locator")
            })?;
        let call_label = label
            .split_once(" call")
            .map(|(value, _)| value)
            .ok_or_else(|| format!("IMS row {official_row} has no call label"))?;
        let command_label = command_label
            .split_once(" command")
            .or_else(|| command_label.split_once(" call"))
            .map(|(value, _)| value)
            .ok_or_else(|| format!("IMS row {official_row} has no command label"))?;
        let call_names = names(call_label, &official_row)?;
        let command_names = names(command_label, &official_row)?;
        families.push(Family {
            ordinal,
            official_row,
            label,
            source_locator,
            call_names,
            command_names,
        });
    }
    require(
        families
            .iter()
            .map(|family| family.call_names.len())
            .sum::<usize>()
            == 30,
        "IMS call-name projection must contain 30 family memberships",
    )?;
    require(
        families
            .iter()
            .map(|family| family.command_names.len())
            .sum::<usize>()
            == 30,
        "IMS command-name projection must contain 30 family memberships",
    )?;

    let catalog_sha256 = format!("sha256:{}", file_digest(&source_path)?);
    let topic_sha256 = format!("sha256:{}", text(topic, "sha256", &manifest_path)?);
    let registry_sha256 = registry_digest(&families);
    format_generated_rust(render_source(
        &families,
        &catalog_sha256,
        &topic_sha256,
        &registry_sha256,
    ))
}

fn names(value: &str, row: &str) -> TaskResult<Vec<String>> {
    let names = value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty() && !matches!(*token, "and" | "or"))
        .map(str::to_ascii_uppercase)
        .collect::<Vec<_>>();
    require(
        !names.is_empty()
            && names.iter().all(|name| {
                name.len() <= 8
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
            }),
        &format!("IMS row {row} contains an invalid call or command name"),
    )?;
    Ok(names)
}

fn registry_digest(families: &[Family]) -> String {
    let mut digest = Sha256::new();
    digest.update(REGISTRY_DOMAIN);
    for family in families {
        digest.update([family.ordinal]);
        for value in [
            family.official_row.as_str(),
            family.label.as_str(),
            family.source_locator.as_str(),
        ] {
            digest.update((value.len() as u64).to_be_bytes());
            digest.update(value.as_bytes());
        }
        for names in [&family.call_names, &family.command_names] {
            digest.update((names.len() as u64).to_be_bytes());
            for name in names {
                digest.update((name.len() as u64).to_be_bytes());
                digest.update(name.as_bytes());
            }
        }
    }
    format!("sha256:{:x}", digest.finalize())
}

fn render_source(
    families: &[Family],
    catalog_sha256: &str,
    topic_sha256: &str,
    registry_sha256: &str,
) -> String {
    let mut output = format!(
        "// @generated by `cargo xtask ims-catalog`; do not edit.\n\
         // Catalog identity is not behavioral coverage.\n\n\
         pub const IMS_CALL_BASELINE: &str = {BASELINE:?};\n\
         pub const IMS_CALL_SOURCE_TOPIC: &str = {SOURCE_TOPIC:?};\n\
         pub const IMS_CALL_SOURCE_TOPIC_SHA256: &str = {topic_sha256:?};\n\
         pub const IMS_CALL_CATALOG_SHA256: &str = {catalog_sha256:?};\n\
         pub const IMS_CALL_REGISTRY_SHA256: &str = {registry_sha256:?};\n\
         pub const IMS_CALL_FAMILY_COUNT: usize = {};\n\
         pub const IMS_CALL_NAME_MEMBERSHIPS: usize = 30;\n\
         pub const IMS_COMMAND_NAME_MEMBERSHIPS: usize = 30;\n\n\
         pub const IMS_CALL_FAMILIES: &[ImsCallFamilyDescriptor] = &[\n",
        families.len()
    );
    for family in families {
        output.push_str("    ImsCallFamilyDescriptor {\n");
        output.push_str(&format!("        ordinal: {},\n", family.ordinal));
        output.push_str(&format!(
            "        official_row: {:?},\n",
            family.official_row
        ));
        output.push_str(&format!("        label: {:?},\n", family.label));
        output.push_str(&format!(
            "        source_locator: {:?},\n",
            family.source_locator
        ));
        output.push_str("        call_names: &[");
        for name in &family.call_names {
            output.push_str(&format!("{name:?}, "));
        }
        output.push_str("],\n        command_names: &[");
        for name in &family.command_names {
            output.push_str(&format!("{name:?}, "));
        }
        output.push_str("],\n    },\n");
    }
    output.push_str("];\n");
    output
}

fn render_ssa(root: &Path) -> TaskResult<Vec<u8>> {
    let rules_path = root.join(SSA_RULES_PATH);
    let rules = json(&rules_path)?;
    let schema_path = root.join(SSA_SCHEMA_PATH);
    validate_schema_instance(&json(&schema_path)?, &rules, &rules_path)?;
    require(
        rules["schema_version"] == Value::String("mainframe-env.ims-ssa-rules@1".into())
            && rules["target_version"] == Value::String("0.14.0".into())
            && rules["source_scope"] == Value::String("ims-programming-contracts".into())
            && rules["segment_name_bytes"].as_u64() == Some(8)
            && rules["field_name_bytes"].as_u64() == Some(8)
            && rules["relational_operator_bytes"].as_u64() == Some(2),
        "IMS SSA rule identity or fixed widths drifted",
    )?;

    let manifest_path = root.join(SSA_MANIFEST_PATH);
    let manifest = json(&manifest_path)?;
    let manifest_topics = array(&manifest, "topics", &manifest_path)?;
    let mut cited_topics = BTreeSet::new();
    for source in array(&rules, "source_topics", &rules_path)? {
        let topic_path = text(source, "topic_path", &rules_path)?;
        let sha256 = text(source, "sha256", &rules_path)?;
        require(
            cited_topics.insert(topic_path.to_string())
                && manifest_topics.iter().any(|topic| {
                    topic["topic_path"].as_str() == Some(topic_path)
                        && topic["sha256"].as_str() == Some(sha256)
                }),
            &format!("IMS SSA rules cite an unpinned or duplicate topic {topic_path}"),
        )?;
    }
    require(
        cited_topics.len() == 7,
        "IMS SSA rules must cite the seven reviewed grammar topics",
    )?;
    for group in [
        "names",
        "command_codes",
        "relational_operators",
        "boolean_connectors",
        "concatenated_key",
    ] {
        let source = &rules["rule_groups"][group];
        let topic_path = text(source, "topic_path", &rules_path)?;
        let sha256 = text(source, "sha256", &rules_path)?;
        require(
            cited_topics.contains(topic_path)
                && manifest_topics.iter().any(|topic| {
                    topic["topic_path"].as_str() == Some(topic_path)
                        && topic["sha256"].as_str() == Some(sha256)
                }),
            &format!("IMS SSA {group} rule group cites an unpinned topic"),
        )?;
    }

    let expected_codes = [
        "A", "C", "D", "F", "G", "L", "M", "N", "O", "P", "Q", "R", "S", "U", "V", "W", "Z",
    ];
    let codes = array(&rules, "command_codes", &rules_path)?;
    require(
        codes.len() == expected_codes.len(),
        "IMS SSA command-code denominator drifted",
    )?;
    let mut code_rows = Vec::new();
    for (entry, expected) in codes.iter().zip(expected_codes) {
        let code = text(entry, "code", &rules_path)?;
        let behavior = text(entry, "behavior", &rules_path)?;
        let subset_pointer = entry["subset_pointer"]
            .as_bool()
            .ok_or("IMS SSA subset_pointer is not Boolean")?;
        let dedb_only = entry["dedb_only"]
            .as_bool()
            .ok_or("IMS SSA dedb_only is not Boolean")?;
        require(
            code == expected
                && subset_pointer == matches!(code, "M" | "R" | "S" | "W" | "Z")
                && dedb_only == subset_pointer,
            &format!("IMS SSA command-code metadata drifted for {code}"),
        )?;
        code_rows.push((code, behavior, subset_pointer, dedb_only));
    }

    let expected_relations = [
        "equal",
        "greater-than",
        "less-than",
        "greater-or-equal",
        "less-or-equal",
        "not-equal",
    ];
    let relations = array(&rules, "relational_operators", &rules_path)?;
    require(
        relations.len() == expected_relations.len(),
        "IMS SSA relation denominator drifted",
    )?;
    let mut relation_rows = Vec::new();
    let mut relation_encodings = BTreeSet::new();
    for (entry, expected) in relations.iter().zip(expected_relations) {
        let relation = text(entry, "relation", &rules_path)?;
        require(
            relation == expected,
            &format!("IMS SSA relation ordering drifted for {relation}"),
        )?;
        let encodings = array(entry, "encodings", &rules_path)?
            .iter()
            .map(|encoding| {
                let encoding = encoding
                    .as_str()
                    .ok_or("IMS SSA relation encoding is not text")?;
                let pair = hex_pair(encoding)?;
                require(
                    relation_encodings.insert(pair),
                    &format!("IMS SSA relation encoding {encoding} is duplicated"),
                )?;
                Ok(pair)
            })
            .collect::<TaskResult<Vec<_>>>()?;
        require(
            encodings.len() == 3,
            &format!("IMS SSA relation {relation} must have three encodings"),
        )?;
        relation_rows.push((relation, encodings));
    }

    let expected_connectors = [
        ("dependent-and", 0x2a),
        ("dependent-and", 0x26),
        ("logical-or", 0x2b),
        ("logical-or", 0x7c),
        ("independent-and", 0x23),
    ];
    let connectors = array(&rules, "boolean_connectors", &rules_path)?;
    require(
        connectors.len() == expected_connectors.len(),
        "IMS SSA Boolean connector denominator drifted",
    )?;
    let mut connector_rows = Vec::new();
    for (entry, (expected_name, expected_byte)) in connectors.iter().zip(expected_connectors) {
        let name = text(entry, "connector", &rules_path)?;
        let byte = hex_byte(text(entry, "encoding", &rules_path)?)?;
        require(
            name == expected_name && byte == expected_byte,
            &format!("IMS SSA Boolean connector drifted for {name}"),
        )?;
        connector_rows.push((name, byte));
    }

    let mut source = format!(
        "// @generated by `cargo xtask ims-catalog`; do not edit.\n\
         // Reviewed grammar metadata is not behavioral coverage.\n\n\
         pub const IMS_SSA_RULES_SHA256: &str = {:?};\n\
         pub const IMS_SSA_TOPIC_MANIFEST_SHA256: &str = {:?};\n\
         pub const IMS_SSA_SEGMENT_NAME_BYTES: usize = 8;\n\
         pub const IMS_SSA_FIELD_NAME_BYTES: usize = 8;\n\
         pub const IMS_SSA_RELATIONAL_OPERATOR_BYTES: usize = 2;\n\n\
         pub const IMS_SSA_COMMAND_CODES: &[ImsSsaCommandCodeDescriptor] = &[\n",
        format!("sha256:{}", file_digest(&rules_path)?),
        format!(
            "sha256:{}",
            text(&manifest, "topic_manifest_digest", &manifest_path)?
        )
    );
    for (code, behavior, subset_pointer, dedb_only) in code_rows {
        source.push_str(&format!(
            "    ImsSsaCommandCodeDescriptor {{ code: b'{code}', behavior: {behavior:?}, subset_pointer: {subset_pointer}, dedb_only: {dedb_only} }},\n"
        ));
    }
    source.push_str(
        "];\n\npub const IMS_SSA_RELATIONAL_OPERATORS: &[ImsSsaRelationDescriptor] = &[\n",
    );
    for (relation, encodings) in relation_rows {
        source.push_str(&format!(
            "    ImsSsaRelationDescriptor {{ relation: ImsSsaRelation::{}, encodings: &[",
            rust_variant(relation)
        ));
        for [left, right] in encodings {
            source.push_str(&format!("[0x{left:02X}, 0x{right:02X}], "));
        }
        source.push_str("] },\n");
    }
    source
        .push_str("];\n\npub const IMS_SSA_BOOLEAN_CONNECTORS: &[ImsSsaBooleanDescriptor] = &[\n");
    for (connector, encoding) in connector_rows {
        source.push_str(&format!(
            "    ImsSsaBooleanDescriptor {{ connector: ImsSsaBoolean::{}, encoding: 0x{encoding:02X} }},\n",
            rust_variant(connector)
        ));
    }
    source.push_str("];\n");
    format_generated_rust(source)
}

fn rust_variant(value: &str) -> String {
    value
        .split('-')
        .map(|part| {
            let mut characters = part.chars();
            characters
                .next()
                .map(|first| first.to_ascii_uppercase().to_string() + characters.as_str())
                .unwrap_or_default()
        })
        .collect()
}

fn hex_pair(value: &str) -> TaskResult<[u8; 2]> {
    require(
        value.len() == 4,
        "IMS SSA pair encoding must contain four hex digits",
    )?;
    Ok([hex_byte(&value[..2])?, hex_byte(&value[2..])?])
}

fn hex_byte(value: &str) -> TaskResult<u8> {
    u8::from_str_radix(value, 16).map_err(|_| format!("invalid IMS SSA hex byte {value}"))
}

fn format_generated_rust(source: String) -> TaskResult<Vec<u8>> {
    let mut child = Command::new("rustfmt")
        .args(["--edition", "2024", "--emit", "stdout"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("start rustfmt for generated IMS catalog: {error}"))?;
    child
        .stdin
        .as_mut()
        .ok_or("rustfmt stdin is unavailable")?
        .write_all(source.as_bytes())
        .map_err(|error| format!("write generated IMS catalog to rustfmt: {error}"))?;
    let output = child
        .wait_with_output()
        .map_err(|error| format!("wait for IMS catalog rustfmt: {error}"))?;
    require(
        output.status.success(),
        &format!(
            "rustfmt rejected generated IMS catalog: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    )?;
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_preserve_multi_call_families() {
        assert_eq!(
            names("GU, GN, and GNP", "row").unwrap(),
            ["GU", "GN", "GNP"]
        );
        assert_eq!(names("ROLL or ROLB", "row").unwrap(), ["ROLL", "ROLB"]);
    }

    #[test]
    fn names_reject_non_ascii_or_oversized_identities() {
        assert!(names("TOOLONGCALL", "row").is_err());
        assert!(names("é", "row").is_err());
    }
}
