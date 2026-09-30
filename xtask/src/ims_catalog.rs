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
const PCB_STATUS_RULES_PATH: &str = "conformance/0.14/ims/pcb-status-rules.json";
const PCB_STATUS_SCHEMA_PATH: &str = "conformance/0.14/schemas/ims-pcb-status-rules.schema.json";
const GENERATED_PCB_PATH: &str =
    "crates/contracts/mainframe-env-host-api/src/generated/ims_pcb_masks.rs";
const GENERATED_STATUS_PATH: &str =
    "crates/contracts/mainframe-env-host-api/src/generated/ims_status_codes.rs";
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

#[derive(Clone, Debug)]
enum PcbWidth {
    Fixed(usize),
    OneOf(Vec<usize>),
    Variable,
}

#[derive(Clone, Debug)]
struct PcbField {
    name: String,
    width: PcbWidth,
    semantic_value: String,
    contexts: Vec<String>,
}

#[derive(Clone, Debug)]
struct PcbMask {
    kind: String,
    source_topic: String,
    source_sha256: String,
    contexts: Vec<String>,
    fields: Vec<PcbField>,
}

#[derive(Clone, Debug)]
struct StatusRow {
    code: String,
    categories: Vec<String>,
}

#[derive(Clone, Debug)]
struct StatusContext {
    name: String,
    source_topic: String,
    source_sha256: String,
    pcb_kinds: Vec<String>,
    statuses: Vec<StatusRow>,
}

pub(super) fn generate(root: &Path) -> TaskResult {
    let (generated_pcb, generated_status) = render_pcb_status(root)?;
    for (relative, generated) in [
        (GENERATED_PATH, render(root)?),
        (GENERATED_SSA_PATH, render_ssa(root)?),
        (GENERATED_PCB_PATH, generated_pcb),
        (GENERATED_STATUS_PATH, generated_status),
    ] {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().ok_or("generated IMS path has no parent")?)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        fs::write(&path, generated).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(())
}

pub(super) fn check(root: &Path) -> TaskResult {
    let (generated_pcb, generated_status) = render_pcb_status(root)?;
    for (relative, expected) in [
        (GENERATED_PATH, render(root)?),
        (GENERATED_SSA_PATH, render_ssa(root)?),
        (GENERATED_PCB_PATH, generated_pcb),
        (GENERATED_STATUS_PATH, generated_status),
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

fn render_pcb_status(root: &Path) -> TaskResult<(Vec<u8>, Vec<u8>)> {
    let rules_path = root.join(PCB_STATUS_RULES_PATH);
    let rules = json(&rules_path)?;
    let schema_path = root.join(PCB_STATUS_SCHEMA_PATH);
    validate_schema_instance(&json(&schema_path)?, &rules, &rules_path)?;
    require(
        rules["schema_version"] == Value::String("mainframe-env.ims-pcb-status-rules@1".into())
            && rules["target_version"] == Value::String("0.14.0".into())
            && rules["source_scope"] == Value::String("ims-programming-contracts".into()),
        "IMS PCB/status rule identity drifted",
    )?;

    let manifest_path = root.join(SSA_MANIFEST_PATH);
    let manifest = json(&manifest_path)?;
    let manifest_topics = array(&manifest, "topics", &manifest_path)?;
    let source_topics = array(&rules, "source_topics", &rules_path)?;
    let mut cited_topics = BTreeMap::new();
    for source in source_topics {
        let (topic, sha256) = source_pair(source, &rules_path)?;
        require(
            cited_topics.insert(topic.clone(), sha256.clone()).is_none()
                && manifest_topics.iter().any(|candidate| {
                    candidate["topic_path"].as_str() == Some(topic.as_str())
                        && candidate["sha256"].as_str() == Some(sha256.as_str())
                }),
            &format!("IMS PCB/status rules cite an unpinned or duplicate topic {topic}"),
        )?;
    }
    require(
        cited_topics.len() == 7,
        "IMS PCB/status rules must cite four mask and three status topics",
    )?;

    let expected_execution_contexts = ["db-dc", "dbctl", "dcctl", "db-batch", "tm-batch"];
    require(
        strings(&rules, "execution_contexts", &rules_path)? == expected_execution_contexts,
        "IMS execution-context ordering drifted",
    )?;

    let expected_masks = [
        ("database", 9, &["db-dc", "dbctl", "db-batch"][..]),
        (
            "gsam",
            10,
            &["db-dc", "dbctl", "dcctl", "db-batch", "tm-batch"][..],
        ),
        (
            "io",
            14,
            &["db-dc", "dbctl", "dcctl", "db-batch", "tm-batch"][..],
        ),
        ("alternate", 3, &["db-dc", "dcctl"][..]),
    ];
    let mask_values = array(&rules, "pcb_masks", &rules_path)?;
    require(
        mask_values.len() == expected_masks.len(),
        "IMS PCB mask denominator drifted",
    )?;
    let mut masks = Vec::new();
    for (value, (expected_kind, expected_fields, expected_contexts)) in
        mask_values.iter().zip(expected_masks)
    {
        let kind = text(value, "kind", &rules_path)?.to_string();
        let (source_topic, source_sha256) = source_pair(&value["source_topic"], &rules_path)?;
        let contexts = strings(value, "allowed_contexts", &rules_path)?;
        require(
            kind == expected_kind
                && contexts
                    .iter()
                    .map(String::as_str)
                    .eq(expected_contexts.iter().copied())
                && cited_topics.get(&source_topic) == Some(&source_sha256),
            &format!("IMS {kind} PCB identity, source, or contexts drifted"),
        )?;
        let field_values = array(value, "fields", &rules_path)?;
        require(
            field_values.len() == expected_fields,
            &format!("IMS {kind} PCB field count drifted"),
        )?;
        let mut fields = Vec::new();
        let mut field_names = BTreeSet::new();
        let mut status_fields = 0;
        let mut variable_fields = 0;
        for field in field_values {
            let name = text(field, "field", &rules_path)?.to_string();
            let semantic_value = text(field, "semantic_value", &rules_path)?.to_string();
            let field_contexts = strings(field, "applicable_contexts", &rules_path)?;
            require(
                field_names.insert(name.clone())
                    && field_contexts
                        .iter()
                        .all(|context| contexts.contains(context)),
                &format!("IMS {kind} PCB field {name} is duplicated or has a forbidden context"),
            )?;
            let width = pcb_width(&field["width"], &rules_path)?;
            if name == "status-code" {
                status_fields += 1;
                require(
                    matches!(width, PcbWidth::Fixed(2)),
                    &format!("IMS {kind} status field must be two bytes"),
                )?;
            }
            if !matches!(width, PcbWidth::Fixed(_)) {
                variable_fields += 1;
            }
            fields.push(PcbField {
                name,
                width,
                semantic_value,
                contexts: field_contexts,
            });
        }
        require(
            status_fields == 1
                && variable_fields == usize::from(matches!(kind.as_str(), "database" | "gsam")),
            &format!("IMS {kind} PCB variable/status field closure drifted"),
        )?;
        masks.push(PcbMask {
            kind,
            source_topic,
            source_sha256,
            contexts,
            fields,
        });
    }

    let categories = strings(&rules, "status_categories", &rules_path)?;
    let expected_categories = [
        "exceptional-valid-completed",
        "warning-with-data-completed",
        "warning-no-data-completed",
        "improper-user-specification",
        "system-io-security-error",
        "unavailable-data",
        "lock-timeout",
    ];
    require(
        categories == expected_categories,
        "IMS status category ordering drifted",
    )?;
    let expected_status_contexts = [
        ("database", 87, &["database", "gsam"][..]),
        ("system-service", 50, &["io"][..]),
        ("message", 68, &["io", "alternate"][..]),
    ];
    let status_values = array(&rules, "status_contexts", &rules_path)?;
    require(
        status_values.len() == expected_status_contexts.len(),
        "IMS status-context denominator drifted",
    )?;
    let mut status_contexts = Vec::new();
    let mut distinct_codes = BTreeSet::new();
    let mut memberships = 0_usize;
    for (value, (expected_name, expected_count, expected_pcbs)) in
        status_values.iter().zip(expected_status_contexts)
    {
        let name = text(value, "context", &rules_path)?.to_string();
        let (source_topic, source_sha256) = source_pair(&value["source_topic"], &rules_path)?;
        let pcb_kinds = strings(value, "pcb_kinds", &rules_path)?;
        require(
            name == expected_name
                && value["status_count"].as_u64() == Some(expected_count as u64)
                && pcb_kinds
                    .iter()
                    .map(String::as_str)
                    .eq(expected_pcbs.iter().copied())
                && cited_topics.get(&source_topic) == Some(&source_sha256),
            &format!("IMS {name} status identity, source, count, or PCB applicability drifted"),
        )?;
        let mut statuses = BTreeMap::<String, Vec<String>>::new();
        let mut seen_categories = BTreeSet::new();
        let mut last_category = None;
        for group in array(value, "category_groups", &rules_path)? {
            let category = text(group, "category", &rules_path)?.to_string();
            let position = categories
                .iter()
                .position(|candidate| candidate == &category)
                .ok_or_else(|| format!("unknown IMS status category {category}"))?;
            require(
                seen_categories.insert(category.clone())
                    && last_category.is_none_or(|prior| position > prior),
                &format!("IMS {name} status category {category} is repeated or out of order"),
            )?;
            last_category = Some(position);
            let mut group_codes = BTreeSet::new();
            for code in strings(group, "codes", &rules_path)? {
                validate_status_code(&code)?;
                require(
                    group_codes.insert(code.clone()),
                    &format!("IMS {name} status category {category} repeats code {code:?}"),
                )?;
                statuses.entry(code).or_default().push(category.clone());
            }
        }
        for code in strings(value, "uncategorized_codes", &rules_path)? {
            validate_status_code(&code)?;
            require(
                statuses.insert(code.clone(), Vec::new()).is_none(),
                &format!("IMS {name} uncategorized status {code:?} is already categorized"),
            )?;
        }
        require(
            statuses.len() == expected_count && statuses.contains_key("  "),
            &format!("IMS {name} status closure or blank success drifted"),
        )?;
        let status_rows = statuses
            .into_iter()
            .map(|(code, categories)| {
                distinct_codes.insert(code.clone());
                StatusRow { code, categories }
            })
            .collect::<Vec<_>>();
        memberships += status_rows.len();
        status_contexts.push(StatusContext {
            name,
            source_topic,
            source_sha256,
            pcb_kinds,
            statuses: status_rows,
        });
    }
    require(
        memberships == 205 && distinct_codes.len() == 162,
        "IMS status membership or distinct-code denominator drifted",
    )?;

    let rules_sha256 = format!("sha256:{}", file_digest(&rules_path)?);
    let manifest_sha256 = format!(
        "sha256:{}",
        text(&manifest, "topic_manifest_digest", &manifest_path)?
    );
    Ok((
        format_generated_rust(render_pcb_source(&masks, &rules_sha256, &manifest_sha256))?,
        format_generated_rust(render_status_source(
            &status_contexts,
            memberships,
            distinct_codes.len(),
        ))?,
    ))
}

fn strings(value: &Value, field: &str, path: &Path) -> TaskResult<Vec<String>> {
    array(value, field, path)?
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{} field {field} contains non-text", path.display()))
        })
        .collect()
}

fn source_pair(value: &Value, path: &Path) -> TaskResult<(String, String)> {
    Ok((
        text(value, "topic_path", path)?.to_string(),
        text(value, "sha256", path)?.to_string(),
    ))
}

fn pcb_width(value: &Value, path: &Path) -> TaskResult<PcbWidth> {
    if let Some(width) = value["fixed"].as_u64() {
        return usize::try_from(width)
            .map(PcbWidth::Fixed)
            .map_err(|error| format!("{} has oversized PCB field width: {error}", path.display()));
    }
    if let Some(widths) = value["one_of"].as_array() {
        return widths
            .iter()
            .map(|width| {
                width
                    .as_u64()
                    .ok_or_else(|| format!("{} has non-integer PCB width", path.display()))
                    .and_then(|width| usize::try_from(width).map_err(|error| error.to_string()))
            })
            .collect::<TaskResult<Vec<_>>>()
            .map(PcbWidth::OneOf);
    }
    require(
        value["variable"] == Value::String("key-feedback-area".into()),
        "IMS PCB field width has an unknown form",
    )?;
    Ok(PcbWidth::Variable)
}

fn validate_status_code(code: &str) -> TaskResult {
    require(
        code == "  "
            || (code.len() == 2
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())),
        &format!("invalid IMS status code {code:?}"),
    )
}

fn render_pcb_source(masks: &[PcbMask], rules_sha256: &str, manifest_sha256: &str) -> String {
    let mut source = format!(
        "// @generated by `cargo xtask ims-catalog`; do not edit.\n\
         // Reviewed PCB metadata is not behavioral coverage.\n\n\
         pub const IMS_PCB_STATUS_RULES_SHA256: &str = {rules_sha256:?};\n\
         pub const IMS_PCB_STATUS_TOPIC_MANIFEST_SHA256: &str = {manifest_sha256:?};\n\
         pub const IMS_PCB_MASK_COUNT: usize = {};\n\n\
         pub const IMS_PCB_MASKS: &[ImsPcbMaskDescriptor] = &[\n",
        masks.len()
    );
    for mask in masks {
        source.push_str("    ImsPcbMaskDescriptor {\n");
        source.push_str(&format!(
            "        kind: ImsPcbKind::{},\n        source_topic: {:?},\n        source_sha256: {:?},\n",
            rust_variant(&mask.kind),
            mask.source_topic,
            format!("sha256:{}", mask.source_sha256)
        ));
        source.push_str("        allowed_contexts: &[");
        for context in &mask.contexts {
            source.push_str(&format!("ImsExecutionContext::{}, ", rust_variant(context)));
        }
        source.push_str("],\n        fields: &[\n");
        for field in &mask.fields {
            source.push_str("            ImsPcbFieldDescriptor {\n");
            source.push_str(&format!(
                "                field: ImsPcbField::{},\n",
                rust_variant(&field.name)
            ));
            match &field.width {
                PcbWidth::Fixed(width) => source.push_str(&format!(
                    "                width: ImsPcbFieldWidth::Fixed({width}),\n"
                )),
                PcbWidth::OneOf(widths) => source.push_str(&format!(
                    "                width: ImsPcbFieldWidth::OneOf(&{widths:?}),\n"
                )),
                PcbWidth::Variable => source
                    .push_str("                width: ImsPcbFieldWidth::VariableKeyFeedback,\n"),
            }
            source.push_str(&format!(
                "                semantic_value: ImsPcbSemanticValue::{},\n",
                rust_variant(&field.semantic_value)
            ));
            source.push_str("                applicable_contexts: &[");
            for context in &field.contexts {
                source.push_str(&format!("ImsExecutionContext::{}, ", rust_variant(context)));
            }
            source.push_str("],\n            },\n");
        }
        source.push_str("        ],\n    },\n");
    }
    source.push_str("];\n");
    source
}

fn render_status_source(
    contexts: &[StatusContext],
    memberships: usize,
    distinct_codes: usize,
) -> String {
    let mut source = format!(
        "// @generated by `cargo xtask ims-catalog`; do not edit.\n\
         // Reviewed status metadata is not behavioral coverage.\n\n\
         pub const IMS_STATUS_CONTEXT_MEMBERSHIPS: usize = {memberships};\n\
         pub const IMS_STATUS_DISTINCT_CODE_COUNT: usize = {distinct_codes};\n\n\
         pub const IMS_STATUS_CONTEXTS: &[ImsStatusContextDescriptor] = &[\n"
    );
    for context in contexts {
        source.push_str("    ImsStatusContextDescriptor {\n");
        source.push_str(&format!(
            "        context: ImsStatusContext::{},\n        source_topic: {:?},\n        source_sha256: {:?},\n",
            rust_variant(&context.name),
            context.source_topic,
            format!("sha256:{}", context.source_sha256)
        ));
        source.push_str("        pcb_kinds: &[");
        for kind in &context.pcb_kinds {
            source.push_str(&format!("ImsPcbKind::{}, ", rust_variant(kind)));
        }
        source.push_str("],\n        statuses: &[\n");
        for status in &context.statuses {
            source.push_str(&format!(
                "            ImsStatusDescriptor {{ code: *b{:?}, categories: &[",
                status.code
            ));
            for category in &status.categories {
                source.push_str(&format!("ImsStatusCategory::{}, ", rust_variant(category)));
            }
            source.push_str("] },\n");
        }
        source.push_str("        ],\n    },\n");
    }
    source.push_str("];\n");
    source
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

    #[test]
    fn pcb_status_projection_has_exact_reviewed_denominators() {
        let root = repository_root().unwrap();
        let (pcb, status) = render_pcb_status(&root).unwrap();
        let pcb = String::from_utf8(pcb).unwrap();
        let status = String::from_utf8(status).unwrap();
        assert!(pcb.contains("pub const IMS_PCB_MASK_COUNT: usize = 4;"));
        assert!(status.contains("pub const IMS_STATUS_CONTEXT_MEMBERSHIPS: usize = 205;"));
        assert!(status.contains("pub const IMS_STATUS_DISTINCT_CODE_COUNT: usize = 162;"));
    }

    #[test]
    fn status_codes_accept_only_exact_two_byte_display_forms() {
        for code in ["  ", "AB", "A1", "X9"] {
            assert!(validate_status_code(code).is_ok());
        }
        for code in ["", "A", "AAA", "aB", "A ", "!A"] {
            assert!(validate_status_code(code).is_err());
        }
    }

    #[test]
    fn pcb_status_generation_rejects_mutated_denominator_and_source() {
        let source_root = repository_root().unwrap();
        let root = std::env::temp_dir().join(format!(
            "ims-pcb-catalog-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for relative in [
            PCB_STATUS_RULES_PATH,
            PCB_STATUS_SCHEMA_PATH,
            SSA_MANIFEST_PATH,
        ] {
            let destination = root.join(relative);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::copy(source_root.join(relative), destination).unwrap();
        }

        let rules_path = root.join(PCB_STATUS_RULES_PATH);
        let original = fs::read(&rules_path).unwrap();
        let mut rules: Value = serde_json::from_slice(&original).unwrap();
        rules["status_contexts"][0]["status_count"] = Value::from(88);
        fs::write(&rules_path, serde_json::to_vec(&rules).unwrap()).unwrap();
        assert!(render_pcb_status(&root).is_err());

        rules["status_contexts"][0]["status_count"] = Value::from(87);
        rules["source_topics"][0]["sha256"] = Value::String("0".repeat(64));
        fs::write(&rules_path, serde_json::to_vec(&rules).unwrap()).unwrap();
        assert!(render_pcb_status(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
