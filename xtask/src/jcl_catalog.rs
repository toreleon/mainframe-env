use super::*;

const SOURCE_PATH: &str = "conformance/0.2/catalogs/jcl-jes2.json";
const SEMANTICS_PATH: &str = "conformance/0.7/catalogs/jcl-planner-semantics.json";
const SEMANTICS_SCHEMA_PATH: &str = "conformance/0.7/schemas/jcl-planner-semantics.schema.json";
const PLAN_SCHEMA_PATH: &str = "conformance/0.7/schemas/jcl-job-plan.schema.json";
const GENERATED_JSON_PATH: &str = "conformance/0.7/generated/jcl-catalog.json";
const GENERATED_RUST_PATH: &str = "crates/apps/mainframe-env-batch/src/generated/jcl_catalog.rs";
const GENERATED_DOC_PATH: &str = "docs/generated/jcl-0.7.0-catalog.md";
const GENERATED_SCHEMA_PATH: &str = "conformance/0.7/schemas/jcl-generated-catalog.schema.json";
const CATALOG_CONTRACT: &str = "mainframe-env.jcl-generated-catalog@1";

const FAMILIES: [(&str, usize, &str, &str); 6] = [
    ("jcl-statements", 20, "JclStatementId", "JCL_STATEMENTS"),
    (
        "jes2-jecl-statements",
        13,
        "Jes2StatementId",
        "JES2_STATEMENTS",
    ),
    ("dd-parameters", 74, "DdParameterId", "DD_PARAMETERS"),
    ("exec-parameters", 19, "ExecParameterId", "EXEC_PARAMETERS"),
    ("job-parameters", 35, "JobParameterId", "JOB_PARAMETERS"),
    (
        "output-parameters",
        76,
        "OutputParameterId",
        "OUTPUT_PARAMETERS",
    ),
];

#[derive(Clone, Debug)]
struct Entry {
    family: String,
    ordinal: u16,
    row_id: String,
    label: String,
    source_locator: String,
    variant: String,
    keyword: String,
    aliases: Vec<String>,
    validation: String,
    minimum: Option<u64>,
    maximum: Option<u64>,
    choices: Vec<String>,
    support: String,
    sensitive: bool,
    capability: String,
}

struct Artifacts {
    json: Vec<u8>,
    rust: Vec<u8>,
    documentation: Vec<u8>,
}

pub(super) fn generate(root: &Path) -> TaskResult {
    let artifacts = render(root)?;
    for (relative, bytes) in [
        (GENERATED_JSON_PATH, artifacts.json),
        (GENERATED_RUST_PATH, artifacts.rust),
        (GENERATED_DOC_PATH, artifacts.documentation),
    ] {
        let path = root.join(relative);
        fs::create_dir_all(
            path.parent()
                .ok_or("generated JCL artifact has no parent")?,
        )
        .map_err(|error| format!("{}: {error}", path.display()))?;
        fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(())
}

pub(super) fn check(root: &Path) -> TaskResult {
    let artifacts = render(root)?;
    for (relative, expected) in [
        (GENERATED_JSON_PATH, artifacts.json),
        (GENERATED_RUST_PATH, artifacts.rust),
        (GENERATED_DOC_PATH, artifacts.documentation),
    ] {
        let path = root.join(relative);
        let actual = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        require(
            actual == expected,
            &format!("{relative} is stale; run cargo xtask jcl-catalog"),
        )?;
    }
    Ok(())
}

fn render(root: &Path) -> TaskResult<Artifacts> {
    let source_path = root.join(SOURCE_PATH);
    let source = json(&source_path)?;
    let semantics_path = root.join(SEMANTICS_PATH);
    let semantics = json(&semantics_path)?;
    let semantics_schema_path = root.join(SEMANTICS_SCHEMA_PATH);
    validate_schema_instance(
        &json(&semantics_schema_path)?,
        &semantics,
        &semantics_schema_path,
    )?;
    require(
        source["schema_version"] == Value::String("mainframe-env.official-catalog@1".into())
            && source["baseline_id"] == Value::String("ibm-zos-3.2-jcl-jes2-2026-06".into())
            && source["subsystem"] == Value::String("jcl-jes2".into())
            && source["mandatory_rows"].as_u64() == Some(237),
        "pinned JCL/JES2 catalog identity or denominator drifted",
    )?;
    let units = array(&source, "units", &source_path)?;
    require(
        units.len() == FAMILIES.len(),
        "pinned JCL/JES2 catalog must contain exactly six families",
    )?;
    let mut entries = Vec::new();
    let mut seen_rows = BTreeSet::new();
    for ((expected_family, expected_count, _, _), unit) in FAMILIES.iter().zip(units) {
        let family = text(unit, "id", &source_path)?;
        let rows = array(unit, "rows", &source_path)?;
        require(
            family == *expected_family
                && unit["denominator"].as_u64() == Some(*expected_count as u64)
                && rows.len() == *expected_count,
            &format!("JCL catalog family {expected_family} denominator drifted"),
        )?;
        let mut variants = BTreeSet::new();
        let mut keywords = BTreeSet::new();
        for (index, row) in rows.iter().enumerate() {
            let ordinal = u16::try_from(index + 1).map_err(|error| error.to_string())?;
            let row_id = text(row, "id", &source_path)?.to_string();
            let label = text(row, "label", &source_path)?.to_string();
            let source_locator = text(row, "source_locator", &source_path)?.to_string();
            require(
                row["mandatory"] == Value::Bool(true)
                    && row_id.ends_with(&format!(":{ordinal:04}"))
                    && seen_rows.insert(row_id.clone()),
                &format!("JCL catalog row identity is invalid or duplicated: {row_id}"),
            )?;
            let (variant, keyword, aliases) = normalize(family, &label)?;
            let rule = semantic_rule(&semantics, family, &keyword)?;
            require(
                variants.insert(variant.clone()),
                &format!("JCL generated variant is duplicated in {family}: {variant}"),
            )?;
            for accepted in std::iter::once(&keyword).chain(&aliases) {
                require(
                    keywords.insert(accepted.to_ascii_uppercase()),
                    &format!("JCL generated keyword is duplicated in {family}: {accepted}"),
                )?;
            }
            entries.push(Entry {
                family: family.into(),
                ordinal,
                row_id,
                label,
                source_locator,
                variant,
                keyword,
                aliases,
                validation: rule.validation,
                minimum: rule.minimum,
                maximum: rule.maximum,
                choices: rule.choices,
                support: rule.support,
                sensitive: rule.sensitive,
                capability: rule.capability,
            });
        }
    }
    require(
        entries.len() == 237,
        "JCL generated catalog must contain exactly 237 rows",
    )?;
    let catalog_digest = format!("sha256:{}", file_digest(&source_path)?);
    let semantics_digest = format!("sha256:{}", file_digest(&semantics_path)?);
    let plan_schema_digest = format!("sha256:{}", file_digest(&root.join(PLAN_SCHEMA_PATH))?);
    validate_semantic_rule_closure(&semantics, &entries)?;
    let generated_json = render_json(
        &entries,
        &catalog_digest,
        &semantics_digest,
        &plan_schema_digest,
    )?;
    let generated_value: Value = serde_json::from_slice(&generated_json)
        .map_err(|error| format!("generated JCL catalog projection: {error}"))?;
    let generated_schema_path = root.join(GENERATED_SCHEMA_PATH);
    validate_schema_instance(
        &json(&generated_schema_path)?,
        &generated_value,
        &generated_schema_path,
    )?;
    let generated_digest = format!("sha256:{:x}", Sha256::digest(&generated_json));
    Ok(Artifacts {
        json: generated_json,
        rust: render_rust(
            &entries,
            &catalog_digest,
            &semantics_digest,
            &plan_schema_digest,
            &generated_digest,
        )?,
        documentation: render_documentation(
            &entries,
            &catalog_digest,
            &semantics_digest,
            &plan_schema_digest,
            &generated_digest,
        )
        .into_bytes(),
    })
}

#[derive(Clone, Debug)]
struct SemanticRule {
    validation: String,
    minimum: Option<u64>,
    maximum: Option<u64>,
    choices: Vec<String>,
    support: String,
    sensitive: bool,
    capability: String,
}

fn semantic_rule(semantics: &Value, family: &str, keyword: &str) -> TaskResult<SemanticRule> {
    let family_value = semantics["families"]
        .get(family)
        .ok_or_else(|| format!("JCL semantics omit family {family}"))?;
    let rule = family_value["rules"]
        .get(keyword)
        .unwrap_or(&family_value["default"]);
    let prefix = text(family_value, "capability_prefix", Path::new(SEMANTICS_PATH))?;
    let validation = text(rule, "validation", Path::new(SEMANTICS_PATH))?.to_string();
    let support = text(rule, "support", Path::new(SEMANTICS_PATH))?.to_string();
    let minimum = rule.get("minimum").and_then(Value::as_u64);
    let maximum = rule.get("maximum").and_then(Value::as_u64);
    require(
        minimum
            .zip(maximum)
            .is_none_or(|(minimum, maximum)| minimum <= maximum),
        &format!("JCL semantic range is inverted for {family}/{keyword}"),
    )?;
    let choices = rule
        .get("choices")
        .map(|choices| {
            choices
                .as_array()
                .ok_or_else(|| {
                    format!("JCL semantic choices are not an array: {family}/{keyword}")
                })?
                .iter()
                .map(|choice| {
                    choice.as_str().map(str::to_string).ok_or_else(|| {
                        format!("JCL semantic choice is not a string: {family}/{keyword}")
                    })
                })
                .collect::<TaskResult<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    require(
        (validation == "enum") == !choices.is_empty(),
        &format!("JCL enum choice closure differs for {family}/{keyword}"),
    )?;
    Ok(SemanticRule {
        validation,
        minimum,
        maximum,
        choices,
        support,
        sensitive: rule
            .get("sensitive")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        capability: rule
            .get("capability")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("{prefix}.{}", capability_token(keyword))),
    })
}

fn validate_semantic_rule_closure(semantics: &Value, entries: &[Entry]) -> TaskResult {
    for (family, _, _, _) in FAMILIES {
        let known = entries
            .iter()
            .filter(|entry| entry.family == family)
            .map(|entry| entry.keyword.as_str())
            .collect::<BTreeSet<_>>();
        let rules = semantics["families"][family]["rules"]
            .as_object()
            .ok_or_else(|| format!("JCL semantic rules are missing for {family}"))?;
        let declared = rules.keys().map(String::as_str).collect::<BTreeSet<_>>();
        require(
            declared.is_subset(&known),
            &format!(
                "JCL semantic catalog contains unknown {family} rules: {:?}",
                declared.difference(&known).collect::<Vec<_>>()
            ),
        )?;
    }
    Ok(())
}

fn capability_token(keyword: &str) -> String {
    let value = keyword
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let value = value.trim_matches('-');
    if value.is_empty() {
        "control".into()
    } else {
        value.into()
    }
}

fn normalize(family: &str, label: &str) -> TaskResult<(String, String, Vec<String>)> {
    let result: (&str, &str, Vec<&str>) = match family {
        "jcl-statements" => match label {
            "JCL command" => ("jcl-command", "JCL-COMMAND", vec![]),
            "COMMAND" => ("Command", "COMMAND", vec![]),
            "comment" => ("Comment", "COMMENT", vec!["//*"]),
            "delimiter" => ("Delimiter", "DELIMITER", vec!["/*"]),
            "IF/THEN/ELSE/ENDIF" => ("Conditional", "IF", vec!["THEN", "ELSE", "ENDIF"]),
            "null" => ("Null", "NULL", vec!["//"]),
            "OUTPUT JCL" => ("Output", "OUTPUT", vec![]),
            other => (other, other, vec![]),
        },
        "jes2-jecl-statements" => match label {
            "JES2 command statement" => ("Command", "$", vec!["/*$"]),
            other => {
                let keyword = other
                    .strip_prefix("/*")
                    .and_then(|value| value.strip_suffix(" statement"))
                    .ok_or_else(|| format!("cannot normalize JES2 JECL label {other:?}"))?;
                (keyword, keyword, vec![])
            }
        },
        "dd-parameters" | "exec-parameters" | "job-parameters" | "output-parameters" => match label
        {
            "* Parameter" => ("Asterisk", "*", vec![]),
            "Accounting information parameter" => {
                ("accounting-information", "POSITIONAL-ACCOUNTING", vec![])
            }
            "Programmer's name parameter" => ("programmers-name", "POSITIONAL-PROGRAMMER", vec![]),
            "DSNAME parameter" => ("DSNAME", "DSNAME", vec!["DSN"]),
            "PROC and procedure name parameters" => {
                ("proc-and-procedure-name", "PROC", vec!["PROCEDURE-NAME"])
            }
            "RETAINS and RETAINF parameters" => ("retains-and-retainf", "RETAINS", vec!["RETAINF"]),
            "RETRYL and RETRYT parameters" => ("retryl-and-retryt", "RETRYL", vec!["RETRYT"]),
            other => {
                let keyword = other
                    .strip_suffix(" parameter")
                    .or_else(|| other.strip_suffix(" Parameter"))
                    .or_else(|| other.strip_suffix(" parameters"))
                    .ok_or_else(|| format!("cannot normalize JCL parameter label {other:?}"))?;
                (keyword, keyword, vec![])
            }
        },
        _ => return Err(format!("unknown JCL catalog family {family}")),
    };
    let variant = rust_variant(result.0)?;
    let keyword = result.1.to_ascii_uppercase();
    let aliases = result.2.into_iter().map(str::to_ascii_uppercase).collect();
    Ok((variant, keyword, aliases))
}

fn render_json(
    entries: &[Entry],
    catalog_digest: &str,
    semantics_digest: &str,
    plan_schema_digest: &str,
) -> TaskResult<Vec<u8>> {
    let families = FAMILIES
        .iter()
        .map(|(family, denominator, _, _)| {
            let rows = entries
                .iter()
                .filter(|entry| entry.family == *family)
                .map(|entry| {
                    json!({
                        "row_id": entry.row_id,
                        "ordinal": entry.ordinal,
                        "label": entry.label,
                        "keyword": entry.keyword,
                        "aliases": entry.aliases,
                        "rust_variant": entry.variant,
                        "source_locator": entry.source_locator,
                        "validation": entry.validation,
                        "minimum": entry.minimum,
                        "maximum": entry.maximum,
                        "choices": entry.choices,
                        "support": entry.support,
                        "sensitive": entry.sensitive,
                        "capability": entry.capability,
                    })
                })
                .collect::<Vec<_>>();
            json!({"id": family, "denominator": denominator, "rows": rows})
        })
        .collect::<Vec<_>>();
    pretty_json(&json!({
        "schema_version": CATALOG_CONTRACT,
        "target_version": "0.7.0",
        "source_catalog": SOURCE_PATH,
        "source_catalog_sha256": catalog_digest,
        "planner_semantics": SEMANTICS_PATH,
        "planner_semantics_sha256": semantics_digest,
        "plan_schema": PLAN_SCHEMA_PATH,
        "plan_schema_sha256": plan_schema_digest,
        "generated_coverage_credit": 0,
        "families": families,
    }))
}

fn render_rust(
    entries: &[Entry],
    catalog_digest: &str,
    semantics_digest: &str,
    plan_schema_digest: &str,
    generated_digest: &str,
) -> TaskResult<Vec<u8>> {
    let mut output = String::from("// @generated by `cargo xtask jcl-catalog`; do not edit.\n\n");
    output.push_str(&format!(
        "pub const JCL_OFFICIAL_CATALOG_SHA256: &str = \"{catalog_digest}\";\n"
    ));
    output.push_str(&format!(
        "pub const JCL_PLAN_SCHEMA_SHA256: &str = \"{plan_schema_digest}\";\n"
    ));
    output.push_str(&format!(
        "pub const JCL_PLANNER_SEMANTICS_SHA256: &str = \"{semantics_digest}\";\n"
    ));
    output.push_str(&format!(
        "pub const JCL_GENERATED_CATALOG_SHA256: &str = \"{generated_digest}\";\n\n"
    ));
    output.push_str(
        "#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]\n\
         pub enum JclCatalogSupport { Available, Deferred }\n\n\
         #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]\n\
         pub enum JclValueShape {\n\
             None, JclValue, Text, Name, NameList, Integer, IntegerOrTuple,\n\
             IntegerOrX, IntegerOrSuffix, Boolean, Enum, Size, SizeOrTuple,\n\
             Time, Class, Condition, MessageLevel, Dataset, Disposition,\n\
             Delimiter, RecordFormat, Path, Program, Procedure, Restart, Sysout,\n\
             OutputReference, BackwardReference, Secret,\n\
         }\n\n\
         #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]\n\
         pub struct JclCatalogEntry<I> {\n\
             pub id: I,\n\
             pub row_id: &'static str,\n\
             pub family: &'static str,\n\
             pub ordinal: u16,\n\
             pub label: &'static str,\n\
             pub keyword: &'static str,\n\
             pub aliases: &'static [&'static str],\n\
             pub source_locator: &'static str,\n\
             pub validation: JclValueShape,\n\
             pub minimum: Option<u64>,\n\
             pub maximum: Option<u64>,\n\
             pub choices: &'static [&'static str],\n\
             pub support: JclCatalogSupport,\n\
             pub sensitive: bool,\n\
             pub capability: &'static str,\n\
         }\n\n\
         impl<I: Copy> JclCatalogEntry<I> {\n\
             #[must_use]\n\
             pub fn generated_identity(&self) -> crate::JclGeneratedIdentity {\n\
                 crate::JclGeneratedIdentity::new(\n\
                     self.row_id.into(), self.family.into(), self.ordinal, self.keyword.into(),\n\
                 )\n\
             }\n\
         }\n\n",
    );
    for (family, _, enum_name, static_name) in FAMILIES {
        let family_entries = entries
            .iter()
            .filter(|entry| entry.family == family)
            .collect::<Vec<_>>();
        output.push_str("#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]\n");
        output.push_str(&format!("pub enum {enum_name} {{\n"));
        for entry in &family_entries {
            output.push_str(&format!("    {},\n", entry.variant));
        }
        output.push_str("}\n\n");
        output.push_str(&format!(
            "pub static {static_name}: &[JclCatalogEntry<{enum_name}>] = &[\n"
        ));
        for entry in &family_entries {
            output.push_str("    JclCatalogEntry {\n");
            output.push_str(&format!("        id: {enum_name}::{},\n", entry.variant));
            output.push_str(&format!("        row_id: {:?},\n", entry.row_id));
            output.push_str(&format!("        family: {:?},\n", entry.family));
            output.push_str(&format!("        ordinal: {},\n", entry.ordinal));
            output.push_str(&format!("        label: {:?},\n", entry.label));
            output.push_str(&format!("        keyword: {:?},\n", entry.keyword));
            output.push_str("        aliases: &[");
            for alias in &entry.aliases {
                output.push_str(&format!("{alias:?}, "));
            }
            output.push_str("],\n");
            output.push_str(&format!(
                "        source_locator: {:?},\n",
                entry.source_locator
            ));
            let validation = rust_variant(&entry.validation).expect("validated value shape");
            let support = rust_variant(&entry.support).expect("validated support state");
            output.push_str(&format!(
                "        validation: JclValueShape::{validation},\n"
            ));
            output.push_str(&format!("        minimum: {:?},\n", entry.minimum));
            output.push_str(&format!("        maximum: {:?},\n", entry.maximum));
            output.push_str("        choices: &[");
            for choice in &entry.choices {
                output.push_str(&format!("{choice:?}, "));
            }
            output.push_str("],\n");
            output.push_str(&format!("        support: JclCatalogSupport::{support},\n"));
            output.push_str(&format!("        sensitive: {},\n", entry.sensitive));
            output.push_str(&format!("        capability: {:?},\n", entry.capability));
            output.push_str("    },\n");
        }
        output.push_str("];\n\n");
        output.push_str(&format!(
            "impl {enum_name} {{\n\
                 #[must_use]\n\
                 pub fn descriptor(self) -> &'static JclCatalogEntry<Self> {{\n\
                     {static_name}.iter().find(|entry| entry.id == self).expect(\"generated JCL identity is registered\")\n\
                 }}\n\n\
                 #[must_use]\n\
                 pub fn from_keyword(keyword: &str) -> Option<Self> {{\n\
                     {static_name}.iter().find(|entry| entry.keyword.eq_ignore_ascii_case(keyword) || entry.aliases.iter().any(|alias| alias.eq_ignore_ascii_case(keyword))).map(|entry| entry.id)\n\
                 }}\n\
             }}\n\n"
        ));
    }
    format_generated_rust(output)
}

fn format_generated_rust(source: String) -> TaskResult<Vec<u8>> {
    let mut child = Command::new("rustfmt")
        .args(["--edition", "2024", "--emit", "stdout"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("start rustfmt for generated JCL catalog: {error}"))?;
    child
        .stdin
        .as_mut()
        .ok_or("rustfmt stdin is unavailable")?
        .write_all(source.as_bytes())
        .map_err(|error| format!("write generated JCL catalog to rustfmt: {error}"))?;
    let output = child
        .wait_with_output()
        .map_err(|error| format!("wait for JCL catalog rustfmt: {error}"))?;
    require(
        output.status.success(),
        &format!(
            "rustfmt rejected generated JCL catalog: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    )?;
    Ok(output.stdout)
}

fn render_documentation(
    entries: &[Entry],
    catalog_digest: &str,
    semantics_digest: &str,
    plan_schema_digest: &str,
    generated_digest: &str,
) -> String {
    let mut output = format!(
        "# Generated JCL/JES2 0.7.0 inventory\n\n\
         Generated by `cargo xtask jcl-catalog`; do not edit. Catalog presence grants no semantic coverage.\n\n\
         - Official catalog: `{catalog_digest}`\n\
         - Planner semantics: `{semantics_digest}`\n\
         - Generated catalog: `{generated_digest}`\n\
         - Plan schema: `{plan_schema_digest}`\n\n"
    );
    for (family, denominator, _, _) in FAMILIES {
        output.push_str(&format!("## {family} ({denominator})\n\n"));
        output.push_str("| Ordinal | Keyword | Validation | Support | Capability | Official label | Row ID | Source |\n");
        output.push_str("|---:|---|---|---|---|---|---|---|\n");
        for entry in entries.iter().filter(|entry| entry.family == family) {
            let keywords = std::iter::once(entry.keyword.as_str())
                .chain(entry.aliases.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(", ");
            output.push_str(&format!(
                "| {} | `{}` | `{}` | `{}` | `{}` | {} | `{}` | `{}` |\n",
                entry.ordinal,
                keywords,
                entry.validation,
                entry.support,
                entry.capability,
                entry.label,
                entry.row_id,
                entry.source_locator
            ));
        }
        output.push('\n');
    }
    while output.ends_with("\n\n") {
        output.pop();
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_labels_have_stable_noncolliding_identities() {
        assert_eq!(
            normalize("jcl-statements", "IF/THEN/ELSE/ENDIF").unwrap(),
            (
                "Conditional".into(),
                "IF".into(),
                vec!["THEN".into(), "ELSE".into(), "ENDIF".into()]
            )
        );
        assert_eq!(
            normalize("output-parameters", "RETAINS and RETAINF parameters").unwrap(),
            (
                "RetainsAndRetainf".into(),
                "RETAINS".into(),
                vec!["RETAINF".into()]
            )
        );
    }
}
