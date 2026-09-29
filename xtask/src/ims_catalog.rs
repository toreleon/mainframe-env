use super::*;

const SOURCE_PATH: &str = "conformance/0.2/catalogs/ims.json";
const MANIFEST_PATH: &str = "conformance/0.2/manifests/ims-topics.json";
const GENERATED_PATH: &str =
    "crates/foundation/mainframe-env-ir/src/generated/ims_call_registry.rs";
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
    let generated = render(root)?;
    let path = root.join(GENERATED_PATH);
    fs::create_dir_all(path.parent().ok_or("generated IMS path has no parent")?)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    fs::write(&path, generated).map_err(|error| format!("{}: {error}", path.display()))
}

pub(super) fn check(root: &Path) -> TaskResult {
    let expected = render(root)?;
    let path = root.join(GENERATED_PATH);
    let actual = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    require(
        actual == expected,
        "generated IMS call registry is stale; run cargo xtask ims-catalog",
    )
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
