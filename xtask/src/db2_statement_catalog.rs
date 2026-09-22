use super::*;

const SOURCE_PATH: &str = "conformance/0.2/catalogs/db2.json";
const GENERATED_PATH: &str =
    "crates/providers/mainframe-env-db2/src/generated_statement_catalog.rs";
const BASELINE: &str = "ibm-db2-for-zos-13-2026-08-13";
const FAMILIES: [(&str, usize, &str, &str); 2] = [
    ("sql-statements", 158, "Sql", "sql"),
    ("sql-pl-statements", 16, "SqlPl", "sql-pl"),
];

#[derive(Clone, Debug)]
struct Entry {
    unit: &'static str,
    ordinal: u16,
    row_id: String,
    label: String,
    source_locator: String,
    variant: String,
    semantic_id: String,
}

pub(super) fn generate(root: &Path) -> TaskResult {
    let generated = render(root)?;
    let path = root.join(GENERATED_PATH);
    fs::write(&path, generated).map_err(|error| format!("{}: {error}", path.display()))
}

pub(super) fn check(root: &Path) -> TaskResult {
    let expected = render(root)?;
    let path = root.join(GENERATED_PATH);
    let actual = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    require(
        actual == expected,
        "generated Db2 statement catalog is stale; run cargo xtask db2-statement-catalog",
    )
}

fn render(root: &Path) -> TaskResult<Vec<u8>> {
    let source_path = root.join(SOURCE_PATH);
    let source = json(&source_path)?;
    require(
        source["schema_version"] == Value::String("mainframe-env.official-catalog@1".into())
            && source["baseline_id"] == Value::String(BASELINE.into())
            && source["subsystem"] == Value::String("db2".into())
            && source["mandatory_rows"].as_u64() == Some(174),
        "pinned Db2 catalog identity or denominator drifted",
    )?;
    let units = array(&source, "units", &source_path)?;
    require(
        units.len() == FAMILIES.len(),
        "pinned Db2 catalog must contain exactly two units",
    )?;

    let mut entries = Vec::with_capacity(174);
    let mut rows = BTreeSet::new();
    let mut variants = BTreeSet::new();
    let mut semantic_ids = BTreeSet::new();
    for ((expected_unit, expected_count, prefix, semantic_prefix), unit) in
        FAMILIES.iter().zip(units)
    {
        let unit_id = text(unit, "id", &source_path)?;
        let unit_rows = array(unit, "rows", &source_path)?;
        require(
            unit_id == *expected_unit
                && unit["denominator"].as_u64() == Some(*expected_count as u64)
                && unit_rows.len() == *expected_count,
            &format!("Db2 catalog unit {expected_unit} denominator drifted"),
        )?;
        for (index, row) in unit_rows.iter().enumerate() {
            let ordinal = u16::try_from(index + 1).map_err(|error| error.to_string())?;
            let row_id = text(row, "id", &source_path)?.to_string();
            let label = text(row, "label", &source_path)?.to_string();
            let source_locator = text(row, "source_locator", &source_path)?.to_string();
            let words = normalized_words(&label)?;
            let variant = format!("{prefix}{}", pascal_case(&words));
            let semantic_id = format!("{semantic_prefix}.{}", words.join("-"));
            require(
                row["mandatory"] == Value::Bool(true)
                    && row_id == format!("{BASELINE}:{expected_unit}:{ordinal:04}")
                    && source_locator.starts_with("topic:SSEPEK_13.0.0/")
                    && rows.insert(row_id.clone())
                    && variants.insert(variant.clone())
                    && semantic_ids.insert(semantic_id.clone()),
                &format!("Db2 catalog row is invalid or duplicated: {row_id}"),
            )?;
            entries.push(Entry {
                unit: prefix,
                ordinal,
                row_id,
                label,
                source_locator,
                variant,
                semantic_id,
            });
        }
    }
    require(
        entries.len() == 174 && rows.len() == 174,
        "generated Db2 statement catalog must contain exactly 174 rows",
    )?;

    let digest = format!("sha256:{}", file_digest(&source_path)?);
    Ok(render_rust(&entries, &digest).into_bytes())
}

fn normalized_words(label: &str) -> TaskResult<Vec<String>> {
    let words = label
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    require(
        !words.is_empty(),
        &format!("Db2 statement label has no identity words: {label:?}"),
    )?;
    Ok(words)
}

fn pascal_case(words: &[String]) -> String {
    let mut output = String::new();
    for word in words {
        let mut characters = word.chars();
        if let Some(first) = characters.next() {
            output.push(first.to_ascii_uppercase());
            output.extend(characters);
        }
    }
    output
}

fn rust_string(value: &str) -> String {
    format!("{value:?}")
}

fn render_rust(entries: &[Entry], source_digest: &str) -> String {
    let mut output =
        String::from("// @generated by `cargo xtask db2-statement-catalog`; do not edit.\n\n");
    output.push_str("#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]\n");
    output.push_str("pub enum Db2StatementUnit {\n    Sql,\n    SqlPl,\n}\n\n");
    output.push_str("#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]\n");
    output.push_str("pub enum Db2StatementId {\n");
    for entry in entries {
        output.push_str(&format!("    {},\n", entry.variant));
    }
    output.push_str("}\n\n#[rustfmt::skip]\nimpl Db2StatementId {\n");
    output.push_str("    #[must_use]\n    pub const fn as_str(self) -> &'static str {\n");
    output.push_str("        match self {\n");
    for entry in entries {
        output.push_str(&format!(
            "            Self::{} => {},\n",
            entry.variant,
            rust_string(&entry.semantic_id)
        ));
    }
    output.push_str("        }\n    }\n}\n\n");
    output.push_str("#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n");
    output.push_str("pub struct Db2StatementDescriptor {\n");
    output.push_str("    pub id: Db2StatementId,\n");
    output.push_str("    pub unit: Db2StatementUnit,\n");
    output.push_str("    pub ordinal: u16,\n");
    output.push_str("    pub label: &'static str,\n");
    output.push_str("    pub official_row: &'static str,\n");
    output.push_str("    pub source_locator: &'static str,\n");
    output.push_str("}\n\n");
    output.push_str(&format!(
        "#[rustfmt::skip]\npub const DB2_OFFICIAL_STATEMENT_CATALOG_SHA256: &str = {};\n\n",
        rust_string(source_digest)
    ));
    output.push_str("#[rustfmt::skip]\n");
    output.push_str("pub static DB2_STATEMENT_DESCRIPTORS: &[Db2StatementDescriptor] = &[\n");
    for entry in entries {
        output.push_str(&format!(
            "    Db2StatementDescriptor {{ id: Db2StatementId::{}, unit: Db2StatementUnit::{}, ordinal: {}, label: {}, official_row: {}, source_locator: {} }},\n",
            entry.variant,
            entry.unit,
            entry.ordinal,
            rust_string(&entry.label),
            rust_string(&entry.row_id),
            rust_string(&entry.source_locator),
        ));
    }
    output.push_str(
        "];

#[must_use]
pub fn db2_statement_descriptor(id: Db2StatementId) -> &'static Db2StatementDescriptor {
    &DB2_STATEMENT_DESCRIPTORS[id_index(id)]
}

#[must_use]
pub fn db2_statement_descriptor_by_row(
    official_row: &str,
) -> Option<&'static Db2StatementDescriptor> {
    DB2_STATEMENT_DESCRIPTORS
        .iter()
        .find(|descriptor| descriptor.official_row == official_row)
}

const fn id_index(id: Db2StatementId) -> usize {
    id as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_statement_catalog_is_exhaustive_and_ordered() {
        assert_eq!(DB2_STATEMENT_DESCRIPTORS.len(), 174);
        assert_eq!(
            DB2_STATEMENT_DESCRIPTORS
                .iter()
                .filter(|descriptor| descriptor.unit == Db2StatementUnit::Sql)
                .count(),
            158
        );
        assert_eq!(
            DB2_STATEMENT_DESCRIPTORS
                .iter()
                .filter(|descriptor| descriptor.unit == Db2StatementUnit::SqlPl)
                .count(),
            16
        );
        for descriptor in DB2_STATEMENT_DESCRIPTORS {
            assert_eq!(db2_statement_descriptor(descriptor.id), descriptor);
            assert_eq!(
                db2_statement_descriptor_by_row(descriptor.official_row),
                Some(descriptor)
            );
        }
    }
}
",
    );
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        let root = env::temp_dir().join(format!(
            "mainframe-env-db2-statement-catalog-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        let target = root.join(SOURCE_PATH);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::copy(repository_root().unwrap().join(SOURCE_PATH), target).unwrap();
        root
    }

    #[test]
    fn labels_have_stable_owned_identities() {
        assert_eq!(
            normalized_words("ALTER FUNCTION (compiled SQL scalar)").unwrap(),
            ["alter", "function", "compiled", "sql", "scalar"]
        );
        assert_eq!(
            pascal_case(&normalized_words("GET DIAGNOSTICS statement").unwrap()),
            "GetDiagnosticsStatement"
        );
    }

    #[test]
    fn repository_catalog_renders_all_rows() {
        let root = repository_root().unwrap();
        let rendered = String::from_utf8(render(&root).unwrap()).unwrap();
        assert_eq!(
            rendered.matches("    Db2StatementDescriptor { id:").count(),
            174
        );
        assert!(rendered.contains("pub enum Db2StatementId"));
        assert!(rendered.contains("SqlSelect"));
        assert!(rendered.contains("SqlPlCompoundStatement"));
    }

    #[test]
    fn denominator_and_identity_drift_fail_closed() {
        let root = fixture("drift");
        let path = root.join(SOURCE_PATH);
        let original = json(&path).unwrap();

        let mut changed = original.clone();
        changed["mandatory_rows"] = json!(173);
        fs::write(&path, serde_json::to_vec_pretty(&changed).unwrap()).unwrap();
        assert!(render(&root).unwrap_err().contains("denominator drifted"));

        let mut changed = original;
        let duplicate = changed["units"][0]["rows"][0]["id"].clone();
        changed["units"][0]["rows"][1]["id"] = duplicate;
        fs::write(&path, serde_json::to_vec_pretty(&changed).unwrap()).unwrap();
        assert!(render(&root).unwrap_err().contains("invalid or duplicated"));

        fs::remove_dir_all(root).unwrap();
    }
}
