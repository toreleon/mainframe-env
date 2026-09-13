use super::{Clauses, complete_data_reference};
use crate::{CobolUsage, SemanticModel};
use std::collections::BTreeSet;

pub(super) fn validate(
    clauses: &Clauses,
    present: &BTreeSet<&str>,
    semantic: &SemanticModel,
) -> Result<(), String> {
    if present.len() > 16 {
        return Err(format!(
            "CICS ASSIGN permits at most 16 options, found {}",
            present.len()
        ));
    }
    for name in [
        "APPLICATION",
        "APPLID",
        "CHANNEL",
        "CWALENG",
        "INITPARM",
        "INITPARMLEN",
        "MAJORVERSION",
        "MICROVERSION",
        "MINORVERSION",
        "OPERATION",
        "OPERKEYS",
        "OPSECURITY",
        "PLATFORM",
        "PROGRAM",
        "RESTART",
        "SYSID",
        "TASKPRIORITY",
        "TCTUALENG",
        "TWALENG",
        "USERID",
    ] {
        let Some(value) = clauses.get(name) else {
            continue;
        };
        let target = complete_data_reference(value, semantic)
            .map_err(|_| format!("CICS ASSIGN option {name} requires one writable data area"))?;
        super::super::require_writable(&target)
            .map_err(|_| format!("CICS ASSIGN option {name} requires one writable data area"))?;
        if name == "TASKPRIORITY"
            && (target.usage != CobolUsage::Binary || target.length != 2 || target.scale != 0)
        {
            return Err("CICS ASSIGN TASKPRIORITY requires a halfword binary data area".into());
        }
        if matches!(name, "MAJORVERSION" | "MICROVERSION" | "MINORVERSION")
            && (target.usage != CobolUsage::Binary || target.length != 4 || target.scale != 0)
        {
            return Err(format!(
                "CICS ASSIGN {name} requires a fullword binary data area"
            ));
        }
        if matches!(name, "CWALENG" | "INITPARMLEN" | "TCTUALENG" | "TWALENG")
            && (target.usage != CobolUsage::Binary || target.length != 2 || target.scale != 0)
        {
            return Err(format!(
                "CICS ASSIGN {name} requires a halfword binary data area"
            ));
        }
        if (name == "INITPARM" && target.length != 60)
            || (name == "OPERKEYS" && target.length != 8)
            || (name == "OPSECURITY" && target.length != 3)
            || (name == "PROGRAM" && target.length != 8)
            || (name == "RESTART" && target.length != 1)
        {
            return Err(format!(
                "CICS ASSIGN {name} requires an exact-width data area"
            ));
        }
    }
    Ok(())
}
