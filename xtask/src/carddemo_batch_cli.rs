//! Current CardDemo base-batch command emits its validated additive companion.
use crate::TaskResult;
use mainframe_env_conformance::{
    validate_carddemo_base_batch_evidence, verify_carddemo_base_batch_evidence_from_env,
    verify_carddemo_base_batch_from_env,
};
use serde_json::json;
use std::path::Path;

pub(super) fn check_carddemo_base_batch_evidence(root: &Path) -> TaskResult {
    let evidence = verify_carddemo_base_batch_evidence_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    let credit =
        validate_carddemo_base_batch_evidence(&evidence).map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "evidence": evidence, "local_assurance": credit
        }))
        .map_err(|error| error.to_string())?
    );
    Ok(())
}

/// Preserve the existing command's bare v1 receipt representation.
pub(super) fn check_carddemo_base_batch(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_base_batch_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}
