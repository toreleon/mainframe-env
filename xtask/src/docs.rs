use clap::Command as ClapCommand;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

type Result<T = ()> = std::result::Result<T, String>;

const REGISTRY_PATH: &str = "docs/documentation-registry.json";
const MANIFEST_PATH: &str = "docs/generated/documentation-manifest.json";
const PORTAL_PATH: &str = "docs/README.md";
const NAVIGATION_BEGIN: &str = "<!-- BEGIN GENERATED DOCUMENTATION NAVIGATION -->";
const NAVIGATION_END: &str = "<!-- END GENERATED DOCUMENTATION NAVIGATION -->";

#[derive(Clone, Debug, Eq, PartialEq)]
struct Metadata {
    status: String,
    owner: String,
    scope: String,
    applies_from: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NavigationGroup {
    heading: String,
    entries: Vec<(String, String)>,
    note: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PackageTopology {
    authority: String,
    package_map: String,
    package_count: usize,
}

#[derive(Debug)]
struct Registry {
    source: Value,
    normative: BTreeSet<String>,
    navigation: Vec<NavigationGroup>,
    topology: PackageTopology,
}

pub(crate) fn run(root: &Path, check: bool, command: &ClapCommand) -> Result {
    let registry = registry(root)?;
    let mut documents = markdown_documents(root)?;
    validate_registry(root, &registry, &documents)?;

    let navigation = render_navigation(&registry.navigation)?;
    let portal = documents
        .get(PORTAL_PATH)
        .ok_or("documentation portal is not tracked")?
        .clone();
    let expected_portal = replace_navigation(&portal, &navigation)?;
    documents.insert(PORTAL_PATH.into(), expected_portal.clone());

    let generated_targets = if check {
        BTreeSet::new()
    } else {
        BTreeSet::from([root.join(MANIFEST_PATH)])
    };
    validate_links(root, &documents, &generated_targets)?;
    let command_count = validate_xtask_commands(&documents, command)?;
    let manifest = manifest(root, &registry, &documents, command_count)?;

    if check {
        require_equal(
            &fs::read(root.join(MANIFEST_PATH))
                .map_err(|error| format!("{MANIFEST_PATH}: {error}"))?,
            &manifest,
            "documentation manifest is stale; run `cargo xtask docs`",
        )?;
        require_equal(
            portal.as_bytes(),
            expected_portal.as_bytes(),
            "documentation navigation is stale; run `cargo xtask docs`",
        )
    } else {
        fs::create_dir_all(
            root.join(MANIFEST_PATH)
                .parent()
                .ok_or("documentation manifest has no parent")?,
        )
        .map_err(|error| format!("documentation manifest directory: {error}"))?;
        fs::write(root.join(MANIFEST_PATH), manifest)
            .map_err(|error| format!("{MANIFEST_PATH}: {error}"))?;
        fs::write(root.join(PORTAL_PATH), expected_portal)
            .map_err(|error| format!("{PORTAL_PATH}: {error}"))
    }
}

fn registry(root: &Path) -> Result<Registry> {
    let bytes =
        fs::read(root.join(REGISTRY_PATH)).map_err(|error| format!("{REGISTRY_PATH}: {error}"))?;
    let source: Value =
        serde_json::from_slice(&bytes).map_err(|error| format!("{REGISTRY_PATH}: {error}"))?;
    if source["schema_version"] != "mainframe-env.documentation-registry@1"
        || source["manifest_path"] != MANIFEST_PATH
        || source["portal_path"] != PORTAL_PATH
    {
        return Err("documentation registry identity or output paths are invalid".into());
    }
    let normative_values = source["normative_documents"]
        .as_array()
        .ok_or("documentation registry omits normative_documents")?;
    let mut normative = BTreeSet::new();
    for value in normative_values {
        let path = value
            .as_str()
            .ok_or("documentation registry contains a non-string normative path")?;
        validate_repository_path(path)?;
        if !normative.insert(path.to_string()) {
            return Err(format!("documentation registry repeats {path}"));
        }
    }
    let topology_value = &source["package_topology"];
    let topology = PackageTopology {
        authority: required_text(topology_value, "authority")?.into(),
        package_map: required_text(topology_value, "package_map")?.into(),
        package_count: topology_value["package_count"]
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| *value != 0)
            .ok_or("documentation registry has invalid package_topology.package_count")?,
    };
    validate_repository_path(&topology.authority)?;
    validate_repository_path(&topology.package_map)?;

    let groups = source["navigation"]
        .as_array()
        .ok_or("documentation registry omits navigation")?;
    let mut navigation = Vec::new();
    let mut headings = BTreeSet::new();
    for group in groups {
        let heading = required_text(group, "heading")?;
        if heading.contains('\n') || !headings.insert(heading.to_string()) {
            return Err(format!(
                "documentation navigation heading is invalid: {heading}"
            ));
        }
        let rows = group["entries"]
            .as_array()
            .ok_or_else(|| format!("documentation navigation {heading} omits entries"))?;
        if rows.is_empty() {
            return Err(format!("documentation navigation {heading} is empty"));
        }
        let mut entries = Vec::new();
        let mut paths = BTreeSet::new();
        for row in rows {
            let label = required_text(row, "label")?;
            let path = required_text(row, "path")?;
            validate_repository_path(path)?;
            if label.contains(['\n', '[', ']']) || !paths.insert(path.to_string()) {
                return Err(format!(
                    "documentation navigation entry is invalid: {label} -> {path}"
                ));
            }
            entries.push((label.into(), path.into()));
        }
        let note = group
            .get("note")
            .and_then(Value::as_str)
            .map(str::to_string);
        navigation.push(NavigationGroup {
            heading: heading.into(),
            entries,
            note,
        });
    }
    Ok(Registry {
        source,
        normative,
        navigation,
        topology,
    })
}

fn required_text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("documentation registry omits {key}"))
}

fn validate_repository_path(value: &str) -> Result {
    let path = Path::new(value);
    if value.is_empty()
        || value.contains('\\')
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        Err(format!("documentation registry path is unsafe: {value}"))
    } else {
        Ok(())
    }
}

fn markdown_documents(root: &Path) -> Result<BTreeMap<String, String>> {
    let output = Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            "*.md",
        ])
        .current_dir(root)
        .output()
        .map_err(|error| format!("tracked Markdown discovery: {error}"))?;
    if !output.status.success() {
        return Err("tracked Markdown discovery failed".into());
    }
    let mut documents = BTreeMap::new();
    for raw in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let path = std::str::from_utf8(raw)
            .map_err(|_| "tracked Markdown path is not UTF-8")?
            .to_string();
        validate_repository_path(&path)?;
        let text =
            fs::read_to_string(root.join(&path)).map_err(|error| format!("{path}: {error}"))?;
        documents.insert(path, text);
    }
    if documents.is_empty() {
        return Err("no tracked Markdown documents found".into());
    }
    Ok(documents)
}

fn validate_registry(
    root: &Path,
    registry: &Registry,
    documents: &BTreeMap<String, String>,
) -> Result {
    let discovered = documents
        .keys()
        .filter(|path| automatically_normative(path))
        .cloned()
        .collect::<BTreeSet<_>>();
    let missing = discovered
        .difference(&registry.normative)
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "normative Markdown is absent from the documentation registry: {missing:?}"
        ));
    }
    let mut metadata = BTreeMap::new();
    for path in &registry.normative {
        let text = documents
            .get(path)
            .ok_or_else(|| format!("normative document is not tracked: {path}"))?;
        metadata.insert(path.clone(), normative_metadata(path, text)?);
    }
    let navigated = registry
        .navigation
        .iter()
        .flat_map(|group| group.entries.iter().map(|(_, path)| path.clone()))
        .collect::<BTreeSet<_>>();
    let omitted = registry
        .normative
        .difference(&navigated)
        .cloned()
        .collect::<Vec<_>>();
    if !omitted.is_empty() {
        return Err(format!(
            "normative documents are absent from generated navigation: {omitted:?}"
        ));
    }
    for path in navigated {
        if !documents.contains_key(&path) && !root.join(&path).is_file() {
            return Err(format!("navigation target does not exist: {path}"));
        }
    }
    validate_package_topology(root, registry, documents)?;
    Ok(())
}

fn validate_package_topology(
    root: &Path,
    registry: &Registry,
    documents: &BTreeMap<String, String>,
) -> Result {
    if !registry.normative.contains(&registry.topology.authority)
        || !registry.normative.contains(&registry.topology.package_map)
    {
        return Err("package topology authorities must be normative documents".into());
    }
    let packages = workspace_packages(root)?;
    if packages.len() != registry.topology.package_count {
        return Err(format!(
            "documentation registry expects {} workspace packages, found {}",
            registry.topology.package_count,
            packages.len()
        ));
    }
    let authority = documents
        .get(&registry.topology.authority)
        .ok_or("package topology ADR is not tracked")?;
    let package_map = documents
        .get(&registry.topology.package_map)
        .ok_or("package map is not tracked")?;
    for package in &packages {
        if !authority.contains(package) || !package_map.contains(package) {
            return Err(format!(
                "package topology authorities omit workspace package {package}"
            ));
        }
    }
    let count = registry.topology.package_count;
    if !authority.contains(&format!("**{count}**"))
        || !package_map.contains(&format!("workspace contains {count} packages"))
    {
        return Err("package topology count is stale in its public authorities".into());
    }
    Ok(())
}

fn workspace_packages(root: &Path) -> Result<BTreeSet<String>> {
    let workspace_source = fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|error| format!("Cargo.toml: {error}"))?;
    let workspace: toml::Value = workspace_source
        .parse()
        .map_err(|error| format!("Cargo.toml: {error}"))?;
    let members = workspace["workspace"]["members"]
        .as_array()
        .ok_or("Cargo.toml workspace.members is missing")?;
    let mut packages = BTreeSet::new();
    for member in members {
        let member = member
            .as_str()
            .ok_or("Cargo.toml workspace member is not a string")?;
        validate_repository_path(member)?;
        let manifest_path = root.join(member).join("Cargo.toml");
        let manifest_source = fs::read_to_string(&manifest_path)
            .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
        let manifest: toml::Value = manifest_source
            .parse()
            .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
        let package = manifest["package"]["name"]
            .as_str()
            .ok_or_else(|| format!("{} omits package.name", manifest_path.display()))?;
        if !packages.insert(package.to_string()) {
            return Err(format!("workspace repeats package name {package}"));
        }
    }
    Ok(packages)
}

fn automatically_normative(path: &str) -> bool {
    path == "docs/CHARTER.md"
        || path.starts_with("docs/architecture/")
        || path.starts_with("docs/contracts/")
        || path.strip_prefix("docs/decisions/").is_some_and(|name| {
            name.len() > 8
                && name.as_bytes()[..4].iter().all(u8::is_ascii_digit)
                && name.as_bytes()[4] == b'-'
                && name.ends_with(".md")
        })
}

fn normative_metadata(path: &str, text: &str) -> Result<Metadata> {
    let mut fields = BTreeMap::new();
    for line in text.lines().take(16) {
        let line = line.trim().strip_prefix("- ").unwrap_or(line.trim());
        for name in ["Status", "Owner", "Scope", "Applies from"] {
            if let Some(value) = line
                .strip_prefix(name)
                .and_then(|tail| tail.strip_prefix(':'))
            {
                let value = value.trim().trim_matches('*').trim();
                if value.is_empty() || fields.insert(name, value.to_string()).is_some() {
                    return Err(format!("{path} has invalid or repeated {name} metadata"));
                }
            }
        }
    }
    let status = fields
        .remove("Status")
        .ok_or_else(|| format!("{path} omits Status metadata"))?;
    let normalized = status.to_ascii_lowercase();
    if ![
        "accepted",
        "frozen",
        "normative",
        "proposed",
        "implemented",
        "proven",
        "historical",
        "superseded",
    ]
    .iter()
    .any(|prefix| normalized.starts_with(prefix))
    {
        return Err(format!(
            "{path} has unsupported normative status {status:?}"
        ));
    }
    let owner = fields
        .remove("Owner")
        .ok_or_else(|| format!("{path} omits Owner metadata"))?;
    let scope = fields
        .remove("Scope")
        .ok_or_else(|| format!("{path} omits Scope metadata"))?;
    let applies_from = fields
        .remove("Applies from")
        .ok_or_else(|| format!("{path} omits Applies from metadata"))?;
    if !applies_from.starts_with("mainframe-env ")
        || !applies_from.bytes().any(|byte| byte.is_ascii_digit())
    {
        return Err(format!("{path} has invalid Applies from metadata"));
    }
    Ok(Metadata {
        status,
        owner,
        scope,
        applies_from,
    })
}

fn render_navigation(groups: &[NavigationGroup]) -> Result<String> {
    let mut output = String::new();
    for (index, group) in groups.iter().enumerate() {
        if index != 0 {
            output.push('\n');
        }
        output.push_str("## ");
        output.push_str(&group.heading);
        output.push_str("\n\n");
        for (label, path) in &group.entries {
            output.push_str("- [");
            output.push_str(label);
            output.push_str("](");
            output.push_str(&portal_relative(path)?);
            output.push_str(")\n");
        }
        if let Some(note) = &group.note {
            output.push('\n');
            output.push_str(note.trim());
            output.push('\n');
        }
    }
    Ok(output.trim_end().to_string())
}

fn portal_relative(path: &str) -> Result<String> {
    validate_repository_path(path)?;
    Ok(path
        .strip_prefix("docs/")
        .map(str::to_string)
        .unwrap_or_else(|| format!("../{path}")))
}

fn replace_navigation(portal: &str, navigation: &str) -> Result<String> {
    let begin = portal
        .find(NAVIGATION_BEGIN)
        .ok_or("documentation portal omits generated-navigation begin marker")?;
    let tail = &portal[begin + NAVIGATION_BEGIN.len()..];
    let relative_end = tail
        .find(NAVIGATION_END)
        .ok_or("documentation portal omits generated-navigation end marker")?;
    if tail[relative_end + NAVIGATION_END.len()..].contains(NAVIGATION_END)
        || portal[..begin].contains(NAVIGATION_BEGIN)
    {
        return Err("documentation portal repeats a generated-navigation marker".into());
    }
    let end = begin + NAVIGATION_BEGIN.len() + relative_end + NAVIGATION_END.len();
    Ok(format!(
        "{}{}\n{}\n{}{}",
        &portal[..begin],
        NAVIGATION_BEGIN,
        navigation,
        NAVIGATION_END,
        &portal[end..]
    ))
}

fn validate_links(
    root: &Path,
    documents: &BTreeMap<String, String>,
    generated_targets: &BTreeSet<PathBuf>,
) -> Result {
    let canonical_root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let mut anchors = BTreeMap::<PathBuf, BTreeSet<String>>::new();
    for (source, text) in documents {
        for (line, target) in markdown_links(text) {
            if target.starts_with("http://")
                || target.starts_with("https://")
                || target.starts_with("mailto:")
                || target.starts_with("codex://")
            {
                continue;
            }
            if target.contains("{{") || target.contains('<') && !target.starts_with('<') {
                continue;
            }
            let target = target.trim().trim_matches(['<', '>']);
            let (path, fragment) = target
                .split_once('#')
                .map_or((target, None), |(path, fragment)| (path, Some(fragment)));
            let path = path.split('?').next().unwrap_or(path);
            let unresolved = if path.is_empty() {
                root.join(source)
            } else {
                root.join(source)
                    .parent()
                    .ok_or_else(|| format!("{source}:{line}: source has no parent"))?
                    .join(path)
            };
            if !unresolved.exists() && generated_targets.contains(&unresolved) {
                continue;
            }
            let resolved = fs::canonicalize(&unresolved)
                .map_err(|_| format!("{source}:{line}: broken relative link target {target:?}"))?;
            if !resolved.starts_with(&canonical_root) {
                return Err(format!(
                    "{source}:{line}: relative link escapes the repository"
                ));
            }
            if let Some(fragment) = fragment.filter(|value| !value.is_empty()) {
                if !resolved.is_file() {
                    return Err(format!(
                        "{source}:{line}: anchor target is not a file: {target:?}"
                    ));
                }
                let available = if let Some(cached) = anchors.get(&resolved) {
                    cached
                } else {
                    let content = fs::read_to_string(&resolved)
                        .map_err(|error| format!("{}: {error}", resolved.display()))?;
                    let available = markdown_anchors(&content);
                    anchors.insert(resolved.clone(), available);
                    anchors.get(&resolved).expect("inserted anchor set")
                };
                if !available.contains(fragment) {
                    return Err(format!(
                        "{source}:{line}: broken Markdown anchor #{fragment} in {target:?}"
                    ));
                }
            }
        }
    }
    Ok(())
}

fn markdown_links(text: &str) -> Vec<(usize, String)> {
    let mut links = Vec::new();
    for (line_index, line) in text.lines().enumerate() {
        let mut offset = 0;
        while let Some(found) = line[offset..].find("](") {
            let start = offset + found + 2;
            let mut depth = 1_u32;
            let mut end = None;
            for (index, character) in line[start..].char_indices() {
                match character {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(start + index);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let Some(end) = end else {
                break;
            };
            let raw = line[start..end].trim();
            let target = if let Some(rest) = raw.strip_prefix('<') {
                rest.split_once('>').map(|(value, _)| value).unwrap_or(rest)
            } else {
                raw.split_whitespace().next().unwrap_or("")
            };
            if !target.is_empty() {
                links.push((line_index + 1, target.to_string()));
            }
            offset = end + 1;
        }
        let trimmed = line.trim();
        if trimmed.starts_with('[')
            && let Some((_, target)) = trimmed.split_once("]:")
        {
            let target = target.split_whitespace().next().unwrap_or("");
            if !target.is_empty() {
                links.push((line_index + 1, target.trim_matches(['<', '>']).to_string()));
            }
        }
    }
    links
}

fn markdown_anchors(text: &str) -> BTreeSet<String> {
    let mut anchors = BTreeSet::new();
    let mut occurrences = BTreeMap::<String, usize>::new();
    let mut fenced = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        if let Some(heading) = trimmed
            .strip_prefix('#')
            .and_then(|tail| tail.strip_prefix(' ').or_else(|| tail.strip_prefix('#')))
        {
            let heading = heading.trim_start_matches('#').trim();
            let base = github_slug(heading.trim_end_matches('#').trim());
            if !base.is_empty() {
                let count = occurrences.entry(base.clone()).or_default();
                let anchor = if *count == 0 {
                    base
                } else {
                    format!("{base}-{count}")
                };
                *count += 1;
                anchors.insert(anchor);
            }
        }
        for marker in ["id=\"", "id='"] {
            let mut rest = line;
            while let Some(index) = rest.find(marker) {
                let value = &rest[index + marker.len()..];
                let quote = if marker.ends_with('"') { '"' } else { '\'' };
                if let Some(end) = value.find(quote) {
                    anchors.insert(value[..end].to_string());
                    rest = &value[end + 1..];
                } else {
                    break;
                }
            }
        }
    }
    anchors
}

fn github_slug(heading: &str) -> String {
    heading
        .chars()
        .filter_map(|character| {
            if character.is_alphanumeric() || matches!(character, '-' | '_') {
                Some(character.to_ascii_lowercase())
            } else if character.is_whitespace() {
                Some('-')
            } else {
                None
            }
        })
        .collect()
}

fn validate_xtask_commands(
    documents: &BTreeMap<String, String>,
    command: &ClapCommand,
) -> Result<usize> {
    let mut count = 0;
    for (path, text) in documents {
        for (line, snippet) in xtask_snippets(text) {
            validate_xtask_command(command, &snippet)
                .map_err(|problem| format!("{path}:{line}: {problem}"))?;
            count += 1;
        }
    }
    if count == 0 {
        return Err("no documented cargo xtask commands were found".into());
    }
    Ok(count)
}

fn xtask_snippets(text: &str) -> Vec<(usize, String)> {
    let normalized = text.replace("\\\r\n", " ").replace("\\\n", " ");
    let needle = "cargo xtask";
    let mut snippets = Vec::new();
    let mut offset = 0;
    while let Some(found) = normalized[offset..].find(needle) {
        let start = offset + found;
        let line = normalized[..start]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1;
        let line_start = normalized[..start].rfind('\n').map_or(0, |index| index + 1);
        let last_tick = normalized[line_start..start]
            .rfind('`')
            .map(|index| line_start + index);
        let command_start = start + needle.len();
        let end = if last_tick.is_some() {
            normalized[command_start..]
                .find('`')
                .map_or_else(|| normalized.len(), |index| command_start + index)
        } else {
            normalized[command_start..]
                .find('\n')
                .map_or_else(|| normalized.len(), |index| command_start + index)
        };
        snippets.push((line, normalized[command_start..end].replace('\n', " ")));
        offset = end.max(command_start);
        if offset == normalized.len() {
            break;
        }
    }
    snippets
}

fn validate_xtask_command(command: &ClapCommand, snippet: &str) -> Result {
    let tokens = snippet
        .split_whitespace()
        .map(clean_command_token)
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let Some(name) = tokens.first() else {
        return Ok(());
    };
    if name.starts_with('<') || *name == "and" {
        return Ok(());
    }
    let mut selected = command
        .find_subcommand(name)
        .ok_or_else(|| format!("documented xtask subcommand does not exist: {name}"))?;
    let mut positional_seen = false;
    for token in tokens.iter().skip(1) {
        if let Some(option) = token.strip_prefix("--") {
            let option = option.split('=').next().unwrap_or(option);
            if option.is_empty()
                || !selected
                    .get_arguments()
                    .any(|argument| argument.get_long() == Some(option))
            {
                return Err(format!(
                    "documented option --{option} does not exist for xtask {}",
                    selected.get_name()
                ));
            }
            positional_seen = true;
        } else if !positional_seen && let Some(subcommand) = selected.find_subcommand(token) {
            selected = subcommand;
        } else if token.starts_with('-') && token.len() == 2 {
            let option = token.as_bytes()[1] as char;
            if !selected
                .get_arguments()
                .any(|argument| argument.get_short() == Some(option))
            {
                return Err(format!(
                    "documented option -{option} does not exist for xtask {}",
                    selected.get_name()
                ));
            }
            positional_seen = true;
        } else {
            positional_seen = true;
        }
    }
    Ok(())
}

fn clean_command_token(token: &str) -> &str {
    token.trim_matches(|character: char| {
        matches!(
            character,
            '`' | '[' | ']' | '(' | ')' | ',' | ';' | ':' | '\\'
        )
    })
}

fn manifest(
    root: &Path,
    registry: &Registry,
    documents: &BTreeMap<String, String>,
    command_count: usize,
) -> Result<Vec<u8>> {
    let release_source = fs::read_to_string(root.join("release.toml"))
        .map_err(|error| format!("release.toml: {error}"))?;
    let release: toml::Value = release_source
        .parse()
        .map_err(|error| format!("release.toml: {error}"))?;
    let current = release["product"]["version"]
        .as_str()
        .ok_or("release.toml product.version is missing")?;
    let released = release["product"]["released_version"]
        .as_str()
        .ok_or("release.toml product.released_version is missing")?;
    let channel = release["product"]["channel"]
        .as_str()
        .ok_or("release.toml product.channel is missing")?;
    let state = release["product"]["state"]
        .as_str()
        .ok_or("release.toml product.state is missing")?;

    let metadata = registry
        .normative
        .iter()
        .map(|path| {
            let value = normative_metadata(
                path,
                documents
                    .get(path)
                    .ok_or_else(|| format!("normative document is missing: {path}"))?,
            )?;
            Ok((path, value))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let packages = workspace_packages(root)?;
    let rows = documents
        .iter()
        .map(|(path, text)| {
            let mut row = json!({
                "path":path,
                "title":document_title(path, text)?,
                "sha256":format!("sha256:{:x}", Sha256::digest(text.as_bytes())),
                "relative_links":markdown_links(text).len(),
                "xtask_commands":xtask_snippets(text).len(),
                "normative":false
            });
            if let Some(value) = metadata.get(path) {
                row["normative"] = Value::Bool(true);
                row["status"] = Value::String(value.status.clone());
                row["owner"] = Value::String(value.owner.clone());
                row["scope"] = Value::String(value.scope.clone());
                row["applies_from"] = Value::String(value.applies_from.clone());
            }
            Ok(row)
        })
        .collect::<Result<Vec<_>>>()?;
    let registry_bytes =
        fs::read(root.join(REGISTRY_PATH)).map_err(|error| format!("{REGISTRY_PATH}: {error}"))?;
    let value = json!({
        "schema_version":"mainframe-env.documentation-manifest@1",
        "registry":REGISTRY_PATH,
        "registry_sha256":format!("sha256:{:x}", Sha256::digest(registry_bytes)),
        "portal":PORTAL_PATH,
        "public_version_truth":{
            "current":current,
            "released":released,
            "channel":channel,
            "state":state,
            "authorities":[
                "VERSION",
                "Cargo.toml",
                "release.toml",
                "README.md",
                "CHANGELOG.md",
                "conformance/0.2/inventory/versions.json",
                "docs/delivery/coverage-versions/README.md",
                "docs/delivery/coverage-versions/GITHUB-PROJECT.md"
            ]
        },
        "package_topology":{
            "authority":registry.topology.authority,
            "package_map":registry.topology.package_map,
            "package_count":registry.topology.package_count,
            "packages":packages
        },
        "validation":{
            "relative_links_and_anchors":true,
            "xtask_subcommands_and_options":true,
            "normative_metadata":true,
            "generated_navigation":true,
            "public_version_truth":true
        },
        "counts":{
            "markdown_documents":documents.len(),
            "normative_documents":registry.normative.len(),
            "navigation_groups":registry.navigation.len(),
            "xtask_commands":command_count
        },
        "navigation":registry.source["navigation"].clone(),
        "documents":rows
    });
    let mut bytes = serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn document_title<'a>(path: &str, text: &'a str) -> Result<&'a str> {
    text.lines()
        .find_map(|line| line.strip_prefix("# ").map(str::trim))
        .filter(|title| !title.is_empty())
        .ok_or_else(|| format!("{path} has no level-one title"))
}

fn require_equal(actual: &[u8], expected: &[u8], problem: &str) -> Result {
    if actual == expected {
        Ok(())
    } else {
        Err(problem.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{Arg, Command};

    fn command() -> Command {
        Command::new("xtask")
            .subcommand(Command::new("spec").arg(Arg::new("check").long("check")))
            .subcommand(
                Command::new("evidence")
                    .subcommand(Command::new("seal").arg(Arg::new("check").long("check"))),
            )
    }

    #[test]
    fn documented_commands_reject_unknown_subcommands_and_options() {
        let command = command();
        assert!(validate_xtask_command(&command, "spec --check").is_ok());
        assert!(validate_xtask_command(&command, "evidence seal --check").is_ok());
        assert!(validate_xtask_command(&command, "release-certify").is_err());
        assert!(validate_xtask_command(&command, "spec --missing").is_err());
    }

    #[test]
    fn anchors_follow_heading_duplicates_and_ignore_fences() {
        let anchors = markdown_anchors("# Title\n## Same!\n## Same!\n```\n# Hidden\n```\n");
        assert!(anchors.contains("title"));
        assert!(anchors.contains("same"));
        assert!(anchors.contains("same-1"));
        assert!(!anchors.contains("hidden"));
    }

    #[test]
    fn relative_links_require_existing_targets_and_anchors() {
        let root = std::env::temp_dir().join(format!(
            "mainframe-env-doc-links-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("docs/target.md"), "# Present anchor\n").unwrap();
        let valid = BTreeMap::from([(
            "docs/source.md".into(),
            "[target](target.md#present-anchor)".into(),
        )]);
        assert!(validate_links(&root, &valid, &BTreeSet::new()).is_ok());
        let missing_anchor =
            BTreeMap::from([("docs/source.md".into(), "[target](target.md#absent)".into())]);
        assert!(validate_links(&root, &missing_anchor, &BTreeSet::new()).is_err());
        let missing_file =
            BTreeMap::from([("docs/source.md".into(), "[target](missing.md)".into())]);
        assert!(validate_links(&root, &missing_file, &BTreeSet::new()).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn command_extraction_keeps_wrapped_inline_and_shell_continuations() {
        let snippets = xtask_snippets(
            "Run `cargo xtask evidence\nseal --check`.\n```bash\ncargo xtask spec \\\n  --check\n```\n",
        );
        assert_eq!(snippets.len(), 2);
        assert!(snippets[0].1.contains("evidence seal --check"));
        assert!(snippets[1].1.contains("spec    --check"));
    }

    #[test]
    fn normative_metadata_is_required_and_bounded() {
        let complete = "# Contract\nStatus: **Accepted**\nOwner: maintainers\nScope: bounded contract\nApplies from: mainframe-env 0.8.3\n";
        assert!(normative_metadata("docs/contract.md", complete).is_ok());
        assert!(normative_metadata("docs/contract.md", "# Contract\nStatus: Accepted\n").is_err());
    }

    #[test]
    fn workspace_package_inventory_comes_from_member_manifests() {
        let root = std::env::temp_dir().join(format!(
            "mainframe-env-doc-packages-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(root.join("one")).unwrap();
        fs::create_dir_all(root.join("two")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers=[\"one\",\"two\"]\n",
        )
        .unwrap();
        fs::write(root.join("one/Cargo.toml"), "[package]\nname=\"first\"\n").unwrap();
        fs::write(root.join("two/Cargo.toml"), "[package]\nname=\"second\"\n").unwrap();
        assert_eq!(
            workspace_packages(&root).unwrap(),
            BTreeSet::from(["first".into(), "second".into()])
        );
        fs::write(root.join("two/Cargo.toml"), "[package]\nname=\"first\"\n").unwrap();
        assert!(workspace_packages(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn generated_navigation_replaces_one_bounded_portal_region() {
        let portal = format!("before\n{NAVIGATION_BEGIN}\nold\n{NAVIGATION_END}\nafter\n");
        assert_eq!(
            replace_navigation(&portal, "## Current\n\n- entry").unwrap(),
            format!("before\n{NAVIGATION_BEGIN}\n## Current\n\n- entry\n{NAVIGATION_END}\nafter\n")
        );
    }
}
