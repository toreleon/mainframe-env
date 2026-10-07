//! Live CardDemo host, resource, and online integration checks.

use super::*;

const CORPUS_INVENTORY: &str = "conformance/profiles/carddemo/inventory/carddemo-corpus.json";

pub(super) fn check(root: &Path) -> TaskResult {
    let inventory = root.join(CORPUS_INVENTORY);
    let host =
        verify_carddemo_host_operands_from_env(&inventory).map_err(|error| error.to_string())?;
    verify_carddemo_resources_from_env(&inventory).map_err(|error| error.to_string())?;
    verify_carddemo_base_online_from_env(&inventory).map_err(|error| error.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&host).map_err(|error| error.to_string())?
    );
    Ok(())
}

#[cfg(test)]
fn compare_host_transition(historical: &Value, expected: &Value, current: &Value) -> TaskResult {
    require(
        current == expected,
        "0.9 CardDemo host token receipt differs from comparison",
    )?;
    for field in [
        "schema_version",
        "status",
        "corpus_commit",
        "programs_checked",
        "cics_operations",
        "dli_operations",
        "mq_calls",
        "typed_oracle_cases",
    ] {
        require(
            historical[field] == current[field],
            &format!("CardDemo host preservation changed {field}"),
        )?;
    }
    require(
        historical["sql_operations"] == 20 && current["sql_operations"] == 22,
        "CardDemo SQL operation delta differs from CD-008 to 0.9",
    )?;
    let old = historical["opcodes"]
        .as_object()
        .ok_or("CD-008 host opcodes are malformed")?;
    let now = current["opcodes"]
        .as_object()
        .ok_or("0.9 host opcodes are malformed")?;
    let keys = old.keys().chain(now.keys()).collect::<BTreeSet<_>>();
    for key in keys {
        let before = old.get(key).and_then(Value::as_u64).unwrap_or(0);
        let after = now.get(key).and_then(Value::as_u64).unwrap_or(0);
        let prescribed = match key.as_str() {
            "SQL.INCLUDE" => Some((1, 0)),
            "SQL.DECLARE" => Some((0, 2)),
            "SQL.SELECT" => Some((2, 3)),
            _ => None,
        };
        require(
            prescribed.map_or(before == after, |pair| pair == (before, after)),
            &format!("CardDemo host opcode {key} changed outside the SQL comparison"),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_delta_cannot_mask_cics_or_other_sql_drift() {
        let expected = &json!({
            "schema_version":"mainframe-env.carddemo-host-token-receipt@1",
            "status":"pass", "corpus_commit":"0".repeat(40),
            "programs_checked":1, "cics_operations":1, "dli_operations":0,
            "mq_calls":0, "typed_oracle_cases":1, "typed_operands":1,
            "sql_operations":22, "opcodes":{"CICS.READ":1,"SQL.DECLARE":2,"SQL.SELECT":3}
        });
        let mut historical = json!(expected);
        historical["sql_operations"] = json!(20);
        let opcodes = historical["opcodes"].as_object_mut().unwrap();
        opcodes.remove("SQL.DECLARE");
        opcodes.insert("SQL.INCLUDE".into(), json!(1));
        opcodes.insert("SQL.SELECT".into(), json!(2));
        assert!(compare_host_transition(&historical, expected, expected).is_ok());

        let mut cics_drift = historical.clone();
        cics_drift["opcodes"]["CICS.READ"] = json!(28);
        assert!(compare_host_transition(&cics_drift, expected, expected).is_err());
        let mut sql_drift = historical.clone();
        sql_drift["opcodes"]["SQL.DELETE"] = json!(4);
        assert!(compare_host_transition(&sql_drift, expected, expected).is_err());
        let mut current_drift = json!(expected);
        current_drift["typed_operands"] = json!(0);
        assert!(compare_host_transition(&historical, expected, &current_drift).is_err());
    }
}
