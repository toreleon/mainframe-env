use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) const NOTICE_SCHEMA: &str = "mainframe-env.third-party-license-notices@1";

const PRODUCTION_ROOTS: [&str; 2] = ["mainframe-env-cli", "mainframe-env-server"];
const MAX_PRODUCTION_PACKAGES: usize = 4_096;
const MAX_LEGAL_FILES_PER_PACKAGE: usize = 32;
const MAX_PACKAGE_TREE_ENTRIES: usize = 16_384;
const MAX_LEGAL_SEARCH_DEPTH: usize = 4;
const MAX_LEGAL_FILE_BYTES: usize = 1024 * 1024;
const MAX_TOTAL_LEGAL_BYTES: usize = 16 * 1024 * 1024;
const APACHE_2_0_SHA256: &str = "c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4";
const APPROVED_ICU_SHA256: &str =
    "bb0b8f4efd92bb4f2b8b01aca8d81d9b47593e7e15bc65e35553c6f3c4c56f41";

pub(crate) struct ReleaseLicenseReport {
    pub(crate) bytes: Vec<u8>,
    pub(crate) production_packages: usize,
    pub(crate) third_party_packages: usize,
    pub(crate) unique_legal_texts: usize,
}

#[derive(Clone)]
struct Component {
    name: String,
    version: String,
    expression: String,
    source: String,
    text_digests: BTreeSet<String>,
}

struct LegalText {
    bytes: Vec<u8>,
    users: BTreeSet<String>,
    file_names: BTreeSet<String>,
}

pub(crate) fn generate(root: &Path, target: &str) -> Result<ReleaseLicenseReport, String> {
    if target.is_empty()
        || target.len() > 128
        || !target
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err("release license target is invalid".into());
    }
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--all-features",
            "--filter-platform",
            target,
        ])
        .current_dir(root)
        .output()
        .map_err(|error| format!("cargo metadata for release licenses: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata for release licenses failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let metadata: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("release license metadata JSON: {error}"))?;
    generate_from_metadata(root, target, &metadata)
}

fn generate_from_metadata(
    root: &Path,
    target: &str,
    metadata: &Value,
) -> Result<ReleaseLicenseReport, String> {
    let packages = metadata
        .get("packages")
        .and_then(Value::as_array)
        .ok_or("release license metadata has no packages")?;
    let nodes = metadata
        .get("resolve")
        .and_then(|resolve| resolve.get("nodes"))
        .and_then(Value::as_array)
        .ok_or("release license metadata has no resolved dependency graph")?;
    let workspace_members = metadata
        .get("workspace_members")
        .and_then(Value::as_array)
        .ok_or("release license metadata has no workspace members")?
        .iter()
        .map(|member| {
            member
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| "release license workspace member is not a package id".to_string())
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let packages_by_id = packages
        .iter()
        .map(|package| {
            package
                .get("id")
                .and_then(Value::as_str)
                .map(|id| (id.to_string(), package))
                .ok_or_else(|| "release license package has no id".to_string())
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let nodes_by_id = nodes
        .iter()
        .map(|node| {
            node.get("id")
                .and_then(Value::as_str)
                .map(|id| (id.to_string(), node))
                .ok_or_else(|| "release license graph node has no id".to_string())
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;

    let mut pending = packages
        .iter()
        .filter(|package| {
            package
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| PRODUCTION_ROOTS.contains(&name))
                && package
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| workspace_members.contains(id))
        })
        .filter_map(|package| {
            package
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    if pending.len() != PRODUCTION_ROOTS.len() {
        return Err("release license roots are incomplete or ambiguous".into());
    }
    let mut closure = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !closure.insert(id.clone()) {
            continue;
        }
        if closure.len() > MAX_PRODUCTION_PACKAGES {
            return Err("release production dependency closure exceeds its bound".into());
        }
        let node = nodes_by_id
            .get(&id)
            .ok_or_else(|| format!("release dependency graph omits {id}"))?;
        for dependency in node
            .get("deps")
            .and_then(Value::as_array)
            .ok_or("release dependency graph node has no deps")?
        {
            let production_edge = dependency
                .get("dep_kinds")
                .and_then(Value::as_array)
                .is_some_and(|kinds| {
                    kinds.iter().any(|kind| {
                        kind.get("kind").is_none_or(Value::is_null)
                            || kind.get("kind").and_then(Value::as_str) == Some("build")
                    })
                });
            if production_edge {
                pending.push(
                    dependency
                        .get("pkg")
                        .and_then(Value::as_str)
                        .ok_or("release dependency has no package id")?
                        .to_string(),
                );
            }
        }
    }

    let project_license = read_bounded(&root.join("LICENSE"))?;
    let project_notice = read_bounded(&root.join("NOTICE"))?;
    let approved_icu = read_bounded(&root.join("LICENSES/ICU.txt"))?;
    if digest(&project_license) != APACHE_2_0_SHA256 {
        return Err(
            "the tracked project LICENSE is not the approved complete Apache-2.0 text".into(),
        );
    }
    if digest(&approved_icu) != APPROVED_ICU_SHA256 {
        return Err("the tracked ICU text differs from the repository-owner-approved text".into());
    }
    let mut texts = BTreeMap::new();
    let mut total_legal_bytes = 0usize;
    add_text(
        &mut texts,
        &mut total_legal_bytes,
        project_license.clone(),
        "mainframe-env workspace".into(),
        "LICENSE".into(),
    )?;
    add_text(
        &mut texts,
        &mut total_legal_bytes,
        project_notice,
        "mainframe-env workspace".into(),
        "NOTICE".into(),
    )?;

    let mut components = Vec::new();
    let mut icu_components = 0usize;
    for id in &closure {
        let package = packages_by_id
            .get(id)
            .ok_or_else(|| format!("release metadata omits package {id}"))?;
        if workspace_members.contains(id) {
            continue;
        }
        let name = field(package, "name")?;
        let version = field(package, "version")?;
        let expression = field(package, "license")?;
        let source = package
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or("path dependency");
        validate_inline_field(name, "package name")?;
        validate_inline_field(version, "package version")?;
        validate_inline_field(expression, "license expression")?;
        validate_inline_field(source, "package source")?;
        let component_label = format!("{name} {version}");
        let directory = PathBuf::from(field(package, "manifest_path")?)
            .parent()
            .ok_or_else(|| format!("{component_label} manifest has no parent"))?
            .to_path_buf();
        let legal_files = legal_files(package, &directory)?;
        let mut text_digests = BTreeSet::new();
        let mut contains_approved_icu = false;
        let mut has_substantive_text = false;
        for (file_name, path) in legal_files {
            let bytes = resolve_legal_text(
                package,
                &path,
                &read_bounded(&path)?,
                &closure,
                &packages_by_id,
            )?;
            contains_approved_icu |= bytes == approved_icu;
            has_substantive_text |= bytes.len() >= 256;
            let digest = add_text(
                &mut texts,
                &mut total_legal_bytes,
                bytes,
                component_label.clone(),
                file_name,
            )?;
            text_digests.insert(digest);
        }
        if text_digests.is_empty() {
            if name == "crc-catalog" && version == "2.5.0" && expression == "MIT OR Apache-2.0" {
                let digest = add_text(
                    &mut texts,
                    &mut total_legal_bytes,
                    project_license.clone(),
                    component_label.clone(),
                    "LICENSE (Apache-2.0 option selected by mainframe-env)".into(),
                )?;
                text_digests.insert(digest);
                has_substantive_text = true;
            } else {
                return Err(format!(
                    "{component_label} ({expression}) has no complete license or notice text"
                ));
            }
        }
        if !has_substantive_text {
            return Err(format!(
                "{component_label} ({expression}) has only license pointers or summaries"
            ));
        }
        if expression == "ICU" {
            icu_components = icu_components
                .checked_add(1)
                .ok_or("ICU component count overflow")?;
            if name != "decnumber-sys" || version != "0.1.6" || !contains_approved_icu {
                return Err(format!(
                    "unreviewed ICU component or changed ICU text: {component_label}"
                ));
            }
        }
        components.push(Component {
            name: name.into(),
            version: version.into(),
            expression: expression.into(),
            source: source.into(),
            text_digests,
        });
    }
    if icu_components != 1 {
        return Err("the approved decnumber-sys ICU component is missing or duplicated".into());
    }
    components.sort_by(|left, right| {
        (&left.name, &left.version, &left.source).cmp(&(&right.name, &right.version, &right.source))
    });
    let lock = read_bounded(&root.join("Cargo.lock"))?;
    let lock_digest = digest(&lock);
    let mut output = format!(
        "# mainframe-env license and third-party notices\n\nSchema: `{NOTICE_SCHEMA}`\n\nRelease target: `{target}`\n\nCargo.lock SHA-256: `{lock_digest}`\n\nProduction roots: `mainframe-env-cli`, `mainframe-env-server`\n\nDependency scope: target-filtered Cargo normal and build dependencies; development dependencies are excluded.\n\nProduction closure packages: {}\n\nThird-party packages: {}\n\nUnique full license and notice texts: {}\n\nThe mainframe-env workspace is licensed under Apache-2.0. The repository owner approved retaining the locked `decnumber-sys 0.1.6` dependency under its declared ICU license in ADR-0008.\n\n## Third-party production closure\n\n",
        closure.len(),
        components.len(),
        texts.len(),
    );
    for component in &components {
        output.push_str(&format!(
            "- `{} {}` — expression: `{}`; source: `{}`; full-text SHA-256: {}\n",
            component.name,
            component.version,
            component.expression,
            component.source,
            component
                .text_digests
                .iter()
                .map(|value| format!("`{value}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    output.push_str("\n## Full license and notice texts\n\n");
    for (text_digest, text) in &texts {
        output.push_str(&format!(
            "### `sha256:{text_digest}`\n\nApplies to: {}\n\nSource file names: {}\n\n----- BEGIN FULL TEXT sha256:{text_digest} -----\n",
            text.users
                .iter()
                .map(|value| format!("`{value}`"))
                .collect::<Vec<_>>()
                .join(", "),
            text.file_names
                .iter()
                .map(|value| format!("`{value}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        output.push_str(
            std::str::from_utf8(&text.bytes)
                .map_err(|_| format!("legal text sha256:{text_digest} is not UTF-8"))?,
        );
        if !text.bytes.ends_with(b"\n") {
            output.push('\n');
        }
        output.push_str(&format!(
            "----- END FULL TEXT sha256:{text_digest} -----\n\n"
        ));
    }
    Ok(ReleaseLicenseReport {
        bytes: output.into_bytes(),
        production_packages: closure.len(),
        third_party_packages: components.len(),
        unique_legal_texts: texts.len(),
    })
}

fn field<'a>(value: &'a Value, name: &str) -> Result<&'a str, String> {
    value
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("release license package omits {name}"))
}

fn validate_inline_field(value: &str, field_name: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 4_096
        || value
            .chars()
            .any(|character| character.is_control() || character == '`')
    {
        Err(format!("release license {field_name} is invalid"))
    } else {
        Ok(())
    }
}

fn legal_files(package: &Value, directory: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut paths = BTreeSet::new();
    let mut entries = 0usize;
    collect_legal_files(directory, directory, 0, &mut entries, &mut paths)?;
    if let Some(license_file) = package.get("license_file").and_then(Value::as_str) {
        let path = PathBuf::from(license_file);
        let path = if path.is_absolute() {
            path
        } else {
            directory.join(path)
        };
        let canonical_directory = fs::canonicalize(directory)
            .map_err(|error| format!("canonicalize package directory: {error}"))?;
        let canonical_path = fs::canonicalize(&path)
            .map_err(|error| format!("canonicalize package license file: {error}"))?;
        if !canonical_path.starts_with(&canonical_directory) || !canonical_path.is_file() {
            return Err("package license_file escapes its package".into());
        }
        let relative = canonical_path
            .strip_prefix(&canonical_directory)
            .map_err(|_| "package license_file is not relative")?
            .to_str()
            .ok_or("package license_file is not UTF-8")?
            .to_string();
        validate_inline_field(&relative, "file name")?;
        paths.insert((relative, canonical_path));
    }
    if paths.len() > MAX_LEGAL_FILES_PER_PACKAGE {
        return Err("package legal file count exceeds its bound".into());
    }
    Ok(paths.into_iter().collect())
}

fn collect_legal_files(
    root: &Path,
    directory: &Path,
    depth: usize,
    entries: &mut usize,
    paths: &mut BTreeSet<(String, PathBuf)>,
) -> Result<(), String> {
    for entry in fs::read_dir(directory)
        .map_err(|error| format!("read legal files below {}: {error}", directory.display()))?
    {
        *entries = entries
            .checked_add(1)
            .ok_or("package tree entry count overflow")?;
        if *entries > MAX_PACKAGE_TREE_ENTRIES {
            return Err("package tree exceeds the bounded legal-text scan".into());
        }
        let entry = entry.map_err(|error| format!("read package legal file: {error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("inspect package legal file: {error}"))?;
        if file_type.is_dir() && depth < MAX_LEGAL_SEARCH_DEPTH {
            collect_legal_files(root, &entry.path(), depth + 1, entries, paths)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "package legal file name is not UTF-8")?;
        if !is_legal_file_name(&name) {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)
            .map_err(|_| "package legal file is not relative")?
            .to_str()
            .ok_or("package legal file path is not UTF-8")?
            .to_string();
        validate_inline_field(&relative, "file name")?;
        paths.insert((relative, entry.path()));
        if paths.len() > MAX_LEGAL_FILES_PER_PACKAGE {
            return Err("package legal file count exceeds its bound".into());
        }
    }
    Ok(())
}

fn is_legal_file_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    [
        "LICENSE",
        "LICENCE",
        "COPYING",
        "NOTICE",
        "UNLICENSE",
        "COPYRIGHT",
    ]
    .iter()
    .any(|prefix| upper.starts_with(prefix))
}

fn resolve_legal_text(
    package: &Value,
    path: &Path,
    bytes: &[u8],
    closure: &BTreeSet<String>,
    packages_by_id: &BTreeMap<String, &Value>,
) -> Result<Vec<u8>, String> {
    let Some(pointer) = legal_text_pointer(bytes) else {
        return Ok(bytes.to_vec());
    };
    let repository = field(package, "repository")?;
    let version = field(package, "version")?;
    let mut resolutions = BTreeMap::new();
    for id in closure {
        let candidate = packages_by_id
            .get(id)
            .ok_or_else(|| format!("release metadata omits package {id}"))?;
        if candidate.get("repository").and_then(Value::as_str) != Some(repository)
            || candidate.get("version").and_then(Value::as_str) != Some(version)
        {
            continue;
        }
        let directory = PathBuf::from(field(candidate, "manifest_path")?)
            .parent()
            .ok_or("license pointer candidate manifest has no parent")?
            .to_path_buf();
        let candidate_path = directory.join(pointer);
        if !candidate_path.is_file() {
            continue;
        }
        let candidate_bytes = read_bounded(&candidate_path)?;
        if legal_text_pointer(&candidate_bytes).is_some() {
            continue;
        }
        resolutions.insert(digest(&candidate_bytes), candidate_bytes);
    }
    if resolutions.len() != 1 {
        return Err(format!(
            "{} contains unresolved or ambiguous legal-text pointer {}",
            path.display(),
            pointer
        ));
    }
    Ok(resolutions
        .into_values()
        .next()
        .expect("one checked legal-text resolution"))
}

fn legal_text_pointer(bytes: &[u8]) -> Option<&str> {
    let value = std::str::from_utf8(bytes).ok()?.trim();
    let name = value.strip_prefix("../")?;
    (!name.is_empty()
        && !name.contains(['/', '\\'])
        && is_legal_file_name(name)
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')))
    .then_some(name)
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = fs::metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_LEGAL_FILE_BYTES as u64 {
        return Err(format!(
            "{} is not a bounded legal text file",
            path.display()
        ));
    }
    fs::read(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn add_text(
    texts: &mut BTreeMap<String, LegalText>,
    total_legal_bytes: &mut usize,
    bytes: Vec<u8>,
    user: String,
    file_name: String,
) -> Result<String, String> {
    let text_digest = digest(&bytes);
    if !texts.contains_key(&text_digest) {
        *total_legal_bytes = total_legal_bytes
            .checked_add(bytes.len())
            .ok_or("release legal text size overflow")?;
        if *total_legal_bytes > MAX_TOTAL_LEGAL_BYTES {
            return Err("release legal texts exceed their aggregate bound".into());
        }
    }
    let text = texts
        .entry(text_digest.clone())
        .or_insert_with(|| LegalText {
            bytes: bytes.clone(),
            users: BTreeSet::new(),
            file_names: BTreeSet::new(),
        });
    if text.bytes != bytes {
        return Err("legal text SHA-256 collision".into());
    }
    text.users.insert(user);
    text.file_names.insert(file_name);
    Ok(text_digest)
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_production_notices_are_complete_targeted_and_deterministic() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root");
        assert!(generate(root, "../host").is_err());
        let first = generate(root, "aarch64-apple-darwin").expect("first notice generation");
        let second = generate(root, "aarch64-apple-darwin").expect("second notice generation");
        let linux = generate(root, "x86_64-unknown-linux-gnu").expect("Linux notice generation");
        assert_eq!(first.bytes, second.bytes);
        assert_ne!(first.bytes, linux.bytes);
        assert!(first.production_packages > first.third_party_packages);
        assert!(first.third_party_packages > 0);
        assert!(first.unique_legal_texts > 1);
        let notice = std::str::from_utf8(&first.bytes).expect("UTF-8 notice");
        assert!(notice.contains(NOTICE_SCHEMA));
        assert!(notice.contains("Release target: `aarch64-apple-darwin`"));
        assert!(notice.contains("`decnumber-sys 0.1.6` — expression: `ICU`"));
        assert!(notice.contains(
            "Copyright (c) 1995-2005 International Business Machines Corporation and others"
        ));
        assert!(notice.contains("`crc-catalog 2.5.0` — expression: `MIT OR Apache-2.0`"));
        assert!(!notice.contains("- `proptest "));
        assert!(!notice.contains("- `mainframe-env-conformance "));
        assert!(!notice.contains("- `xtask "));
        assert!(!notice.contains("../LICENSE-MIT"));
        assert!(!notice.contains("../LICENSE-APACHE"));
        assert!(
            std::str::from_utf8(&linux.bytes)
                .expect("UTF-8 Linux notice")
                .contains("Release target: `x86_64-unknown-linux-gnu`")
        );
    }
}
