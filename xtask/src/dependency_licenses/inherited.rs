//! Exact, source-pinned MIT texts omitted from selected locked monorepo archives.
use super::*;

pub(super) fn legal_text(
    root: &Path,
    package: &Value,
    directory: &Path,
) -> Result<Option<(Vec<u8>, String)>, String> {
    let name = field(package, "name")?;
    let version = field(package, "version")?;
    let (repository, commit, file, hash) = match (name, version) {
        ("jsonschema-regex" | "jsonschema-value", "0.52.1") => (
            "https://github.com/Stranger6667/jsonschema",
            "94546ceb734c6076e73c4a6723de98804ad63ae6",
            "LICENSES/jsonschema-MIT.txt",
            "117829c3ca21efb132d81a44b55363d395ab8eea18526873bc828da4c0e5f038",
        ),
        ("uuid-simd" | "vsimd", "0.8.0") => (
            "https://github.com/Nugine/simd",
            "d74c030d9dc4f3cae02146d1f497ff62726ef09a",
            "LICENSES/simd-MIT.txt",
            "71674605ec4c087fe9eb534e3e4f9e26eb2e4aabcd76a29fd156c6a844d44b3d",
        ),
        _ => return Ok(None),
    };
    if field(package, "license")? != "MIT" || field(package, "repository")? != repository {
        return Err("inherited MIT license package identity differs".into());
    }
    let vcs: Value =
        serde_json::from_slice(&read_bounded(&directory.join(".cargo_vcs_info.json"))?)
            .map_err(|error| error.to_string())?;
    let bytes = read_bounded(&root.join(file))?;
    if vcs.pointer("/git/sha1").and_then(Value::as_str) != Some(commit) || digest(&bytes) != hash {
        return Err("inherited MIT license source or text identity differs".into());
    }
    Ok(Some((
        bytes,
        format!("{file} (matching upstream monorepo LICENSE)"),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inheritance_is_version_and_repository_scoped() {
        let package = serde_json::json!({"name":"uuid-simd","version":"0.8.1"});
        assert!(
            legal_text(
                Path::new("/nonexistent"),
                &package,
                Path::new("/nonexistent")
            )
            .unwrap()
            .is_none()
        );
        let package = serde_json::json!({"name":"uuid-simd","version":"0.8.0","license":"MIT","repository":"https://attacker.invalid"});
        assert!(
            legal_text(
                Path::new("/nonexistent"),
                &package,
                Path::new("/nonexistent")
            )
            .unwrap_err()
            .contains("package identity")
        );
    }
    #[test]
    fn retained_mit_texts_cannot_be_substituted() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        assert_eq!(
            digest(&read_bounded(&root.join("LICENSES/jsonschema-MIT.txt")).unwrap()),
            "117829c3ca21efb132d81a44b55363d395ab8eea18526873bc828da4c0e5f038"
        );
        assert_eq!(
            digest(&read_bounded(&root.join("LICENSES/simd-MIT.txt")).unwrap()),
            "71674605ec4c087fe9eb534e3e4f9e26eb2e4aabcd76a29fd156c6a844d44b3d"
        );
    }
}
