use super::{HostProblem, pair_operand, validate_vsam_component_clauses};

pub(super) fn cluster_recordsize(statement: &str) -> Result<(u32, u32), HostProblem> {
    validate_vsam_component_clauses(statement)?;
    // #273: IDCAMS KSDS omission default; preserve other cluster defaults.
    let default_recordsize = if statement.contains(" LINEAR")
        || statement.contains("NUMBERED")
        || statement.contains("NONINDEXED")
    {
        (80, 80)
    } else {
        (4089, 4089)
    };
    Ok(pair_operand(statement, "RECORDSIZE").unwrap_or(default_recordsize))
}
