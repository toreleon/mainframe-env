use super::{ProfileIntakeArgs, TaskResult};
use std::{fs, path::Path};

pub(super) fn run(root: &Path, args: &ProfileIntakeArgs) -> TaskResult {
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&args.manifest).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let manifest_schema: serde_json::Value = serde_json::from_str(include_str!(
        "../../conformance/profiles/schemas/profile-corpus.schema.json"
    ))
    .map_err(|e| format!("manifest schema: {e}"))?;
    let validator = jsonschema::validator_for(&manifest_schema).map_err(|e| e.to_string())?;
    if let Err(error) = validator.validate(&manifest) {
        return Err(format!("manifest schema: {error}"));
    }
    let report = mainframe_env_conformance::profile_intake::run(&args.manifest, &args.corpus, root)
        .map_err(|e| e.to_string())?;
    let value = serde_json::to_value(&report).map_err(|e| e.to_string())?;
    let report_schema: serde_json::Value = serde_json::from_str(include_str!(
        "../../conformance/profiles/schemas/profile-intake.schema.json"
    ))
    .map_err(|e| format!("report schema: {e}"))?;
    let validator = jsonschema::validator_for(&report_schema).map_err(|e| e.to_string())?;
    if let Err(error) = validator.validate(&value) {
        return Err(format!("report schema: {error}"));
    }
    let json = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
    let markdown = report.markdown();
    for path in [&args.json, &args.markdown] {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    fs::write(&args.json, json).map_err(|e| e.to_string())?;
    fs::write(&args.markdown, markdown).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn profile_intake_schemas_compile() {
        for source in [
            include_str!("../../conformance/profiles/schemas/profile-corpus.schema.json"),
            include_str!("../../conformance/profiles/schemas/profile-intake.schema.json"),
        ] {
            let schema: serde_json::Value = serde_json::from_str(source).unwrap();
            jsonschema::validator_for(&schema).unwrap();
        }
    }
}
