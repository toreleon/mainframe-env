//! Offline CI schema/freshness gate for the normative reviewed MQ status table.

use crate::{TaskResult, json, require, validate_schema_instance};
use std::path::Path;
use std::process::Command;

const SCHEMA: &str = "conformance/subsystems/mq/schemas/mq-completion-reason-catalog.schema.json";
const CATALOG: &str = "conformance/subsystems/mq/mq/completion-reason-catalog.json";

pub(super) fn check(root: &Path) -> TaskResult {
    let catalog_path = root.join(CATALOG);
    validate_schema_instance(
        &json(&root.join(SCHEMA))?,
        &json(&catalog_path)?,
        &catalog_path,
    )?;
    for tool in [
        "tools/verify_mq_status_catalog.py",
        "tools/generate_mq_status_catalog.py",
    ] {
        let status = Command::new("python3")
            .args(["-B", tool, "--check"])
            .current_dir(root)
            .status()
            .map_err(|error| format!("MQ reviewed-status check: {error}"))?;
        require(
            status.success(),
            &format!("MQ reviewed-status check failed: {tool}"),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reviewed_draft_2020_12_schema_rejects_invalid_status_shapes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let path = root.join(CATALOG);
        let schema = json(&root.join(SCHEMA)).unwrap();
        let catalog = json(&path).unwrap();
        validate_schema_instance(&schema, &catalog, &path).unwrap();
        for mutation in 0..12 {
            let mut value = catalog.clone();
            match mutation {
                0 => value["calls"][0]["pairs"][0]["declared_decimal"] = serde_json::json!(-1),
                1 => value["calls"][0]["pairs"][0]["declared_hex"] = serde_json::json!("GG"),
                2 => value["calls"][4]["pairs"] = value["calls"][0]["pairs"].clone(),
                3 => {
                    value["calls"][0]["pairs"][0]["completion_symbol"] =
                        serde_json::json!("MQCC_UNKNOWN")
                }
                4 => value["calls"][0]["pairs"][0]["review"] = serde_json::json!("pending-number"),
                5 => {
                    value["calls"][0]["pairs"][0]["source_locations"][0]["reason_line"] =
                        serde_json::json!(2001)
                }
                6 => value["licensed_execution_credit"] = serde_json::json!(1),
                7 => {
                    value["completion_wire_mapping"]["classes"][0]["decimal"] =
                        serde_json::json!(-1)
                }
                8 => {
                    value["completion_wire_mapping"]["classes"][0]["decimal"] =
                        serde_json::json!(2147483648_i64)
                }
                9 => {
                    value["completion_wire_mapping"]["classes"][0]["symbol"] =
                        serde_json::json!("MQCC_UNKNOWN")
                }
                10 => {
                    value["completion_numeric_mapping"] =
                        serde_json::json!("pending-not-in-call-pages")
                }
                11 => {
                    value["schema_version"] =
                        serde_json::json!("mainframe-env.mq-completion-reason-catalog@1")
                }
                _ => unreachable!(),
            }
            assert!(
                validate_schema_instance(&schema, &value, &path).is_err(),
                "mutation {mutation}"
            );
        }
    }

    #[test]
    fn reviewed_status_gate_checks_schema_and_freshness_without_external_html() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        check(root).unwrap();
    }
}
