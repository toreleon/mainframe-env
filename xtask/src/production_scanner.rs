//! Reuse the Rust item scanner for production-only assurance checks.

use crate::{TaskResult, require};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const APPLICATION_IDENTITIES: &[&str] = &[
    "CARDDEMO",
    "COBTUPDT",
    "CBPAUP0C",
    "PAUDBLOD",
    "PAUDBUNL",
    "DBPAUTP0",
    "PSBPAUTB",
    "PAUTSUM0",
    "PAUTDTL1",
    "AUTHFRDS",
    "TRANSACTION_TYPE",
    "TRANSACTION_TYPE_CATEGORY",
];

pub(crate) fn read_sources(root: &Path, paths: &[PathBuf]) -> TaskResult<Vec<String>> {
    let sources = scan(
        root,
        "--production-files",
        &serde_json::to_vec(paths).map_err(|error| error.to_string())?,
    )?;
    require(
        sources.len() == paths.len(),
        "production Rust item scanner returned the wrong source count",
    )?;
    Ok(sources)
}

pub(crate) fn read_linked_source(
    root: &Path,
    parent: &Path,
    module: &str,
    exported: &str,
) -> TaskResult<String> {
    let input =
        serde_json::to_vec(&(parent, module, exported)).map_err(|error| error.to_string())?;
    let mut sources = scan(root, "--production-linked-file", &input)?;
    require(
        sources.len() == 1,
        "linked production scanner returned the wrong source count",
    )?;
    sources
        .pop()
        .ok_or_else(|| "linked production scanner returned no source".into())
}

fn scan(root: &Path, mode: &str, input: &[u8]) -> TaskResult<Vec<String>> {
    let tool = root.join("tools/check_typed_semantic_boundaries.py");
    let mut child = Command::new("python3")
        .arg("-B")
        .arg(&tool)
        .arg(mode)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("production Rust item scanner: {error}"))?;
    let write_result = child
        .stdin
        .take()
        .ok_or("production Rust item scanner has no input pipe")?
        .write_all(input);
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    require(
        output.status.success(),
        &format!(
            "production Rust item scanner failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    )?;
    write_result.map_err(|error| format!("production Rust item scanner input: {error}"))?;
    let sources: Vec<String> =
        serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
    Ok(sources)
}

pub(crate) fn read_source(root: &Path, path: &Path) -> TaskResult<String> {
    read_sources(root, &[path.to_path_buf()])?
        .pop()
        .ok_or_else(|| "production Rust item scanner returned no source".into())
}

pub(crate) fn check_application_hardcodes(root: &Path, paths: &[PathBuf]) -> TaskResult {
    let mut hits = Vec::new();
    for (path, production) in paths.iter().zip(read_sources(root, paths)?) {
        let upper = production.to_ascii_uppercase();
        for identity in APPLICATION_IDENTITIES {
            if upper.contains(identity) {
                hits.push(format!(
                    "{}:{identity}",
                    path.strip_prefix(root).unwrap_or(path).display()
                ));
            }
        }
    }
    require(
        hits.is_empty(),
        &format!("production application hardcode scan found {hits:?}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Fixture {
        directory: PathBuf,
        path: PathBuf,
    }

    impl Fixture {
        fn new(source: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let directory = std::env::temp_dir()
                .join(format!("production-scanner-{}-{nonce}", std::process::id()));
            fs::create_dir(&directory).unwrap();
            let path = directory.join("dispatcher.rs");
            fs::write(&path, source).unwrap();
            Self { directory, path }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.directory).unwrap();
        }
    }

    #[test]
    fn inline_test_helper_cannot_hide_later_application_dispatch() {
        let fixture = Fixture::new(
            r#"impl Dispatcher {
    #[cfg(test)]
    fn fixture() { let identity = "CARDDEMO"; }
    fn execute(program: &str) -> bool { program == "COBTUPDT" }
}"#,
        );
        let root = crate::repository_root().unwrap();
        let failure =
            check_application_hardcodes(&root, std::slice::from_ref(&fixture.path)).unwrap_err();
        assert!(failure.contains("COBTUPDT"));
        assert!(!failure.contains("CARDDEMO"));
    }

    #[test]
    fn markers_in_literals_and_comments_cannot_hide_production() {
        let root = crate::repository_root().unwrap();
        for marker in [
            "const MARKER: &str = \"#[cfg(test)]\";",
            "const MARKER: &str = r##\"#![cfg(test)]\"##;",
            "// #[cfg(test)]\n",
            "/* outer /* #[cfg(test)] */ inner */",
        ] {
            let fixture = Fixture::new(&format!(
                "{marker}\nfn execute(program: &str) -> bool {{ program == \"COBTUPDT\" }}"
            ));
            let failure = check_application_hardcodes(&root, std::slice::from_ref(&fixture.path))
                .unwrap_err();
            assert!(failure.contains("COBTUPDT"), "{marker}: {failure}");
        }
    }

    #[test]
    fn legitimate_test_only_identities_are_excluded() {
        let root = crate::repository_root().unwrap();
        let fixture = Fixture::new(
            r#"#[doc = "Test fixtures"]
#[cfg(test)]
mod fixtures { const APPLICATION: &str = "CARDDEMO COBTUPDT"; }
mod nested { #![cfg(test)] const APPLICATION: &str = "PAUDBLOD"; }
#[cfg(test)] pub const SAMPLE: &str = { "PSBPAUTB" };
fn execute(program: &str) -> bool { program.is_empty() }"#,
        );
        check_application_hardcodes(&root, std::slice::from_ref(&fixture.path)).unwrap();
        let source = read_source(&root, &fixture.path).unwrap();
        assert!(source.contains("fn execute"));
    }

    #[test]
    fn missing_or_malformed_source_fails_closed() {
        let root = crate::repository_root().unwrap();
        let fixture = Fixture::new("#[cfg(test)] fn fixture() {");
        assert!(read_source(&root, &fixture.path).is_err());
        fs::remove_file(&fixture.path).unwrap();
        assert!(read_source(&root, &fixture.path).is_err());
    }

    #[test]
    fn linked_trust_excludes_test_configuration_and_requires_actual_link() {
        let root = crate::repository_root().unwrap();
        let fixture = Fixture::new("mod trust;\npub use trust::Authority;");
        let child_directory = fixture.path.with_extension("");
        fs::create_dir(&child_directory).unwrap();
        let child = child_directory.join("trust.rs");
        fs::write(&child, "pub struct Authority;\n#[cfg(test)] mod tests { const KEY: &str = \"MAINFRAME_ENV_PACKAGE_HMAC_KEY_REFS\"; }").unwrap();
        let source = read_linked_source(&root, &fixture.path, "trust", "Authority").unwrap();
        assert!(source.contains("pub struct Authority"));
        assert!(!source.contains("MAINFRAME_ENV_PACKAGE_HMAC_KEY_REFS"));
        fs::write(&child, "pub struct Authority;\nfn environment() { let key = \"MAINFRAME_ENV_PACKAGE_HMAC_KEY_REFS\"; }").unwrap();
        assert!(
            read_linked_source(&root, &fixture.path, "trust", "Authority")
                .unwrap()
                .contains("MAINFRAME_ENV_PACKAGE_HMAC_KEY_REFS")
        );
        for parent in [
            "// mod trust;\npub use trust::Authority;",
            "mod trust;\n#[cfg(test)] pub use trust::Authority;",
            "mod nested { mod trust; pub use trust::Authority; }",
        ] {
            fs::write(&fixture.path, parent).unwrap();
            assert!(read_linked_source(&root, &fixture.path, "trust", "Authority").is_err());
        }
    }
}
