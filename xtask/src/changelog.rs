use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

type Result<T = ()> = std::result::Result<T, String>;

const FRAGMENT_DIRECTORY: &str = "changes/unreleased";
const CHANGELOG_PATH: &str = "CHANGELOG.md";
const SCHEMA_VERSION: &str = "mainframe-env.changelog-fragment@1";
const CATEGORIES: [(&str, &str); 6] = [
    ("added", "Added"),
    ("changed", "Changed"),
    ("deprecated", "Deprecated"),
    ("removed", "Removed"),
    ("fixed", "Fixed"),
    ("security", "Security"),
];

#[derive(Clone, Debug, Eq, PartialEq)]
struct Fragment {
    path: PathBuf,
    category: String,
    summary: String,
}

pub(crate) fn validate(root: &Path) -> Result {
    fragments(root).map(|_| ())
}

pub(crate) fn run(root: &Path, check: bool) -> Result {
    let fragments = fragments(root)?;
    if check || fragments.is_empty() {
        return Ok(());
    }

    let path = root.join(CHANGELOG_PATH);
    let source = fs::read_to_string(&path).map_err(|error| format!("{CHANGELOG_PATH}: {error}"))?;
    let rendered = render(&source, &fragments)?;
    let temporary = root.join("CHANGELOG.md.tmp");
    fs::write(&temporary, rendered).map_err(|error| format!("{}: {error}", temporary.display()))?;
    fs::rename(&temporary, &path).map_err(|error| format!("{CHANGELOG_PATH}: {error}"))?;
    for fragment in fragments {
        fs::remove_file(&fragment.path)
            .map_err(|error| format!("{}: {error}", fragment.path.display()))?;
    }
    Ok(())
}

fn fragments(root: &Path) -> Result<Vec<Fragment>> {
    let directory = root.join(FRAGMENT_DIRECTORY);
    let entries =
        fs::read_dir(&directory).map_err(|error| format!("{FRAGMENT_DIRECTORY}: {error}"))?;
    let mut paths = entries
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>>>()?;
    paths.sort();

    let mut fragments = Vec::new();
    for path in paths {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("{} has a non-UTF-8 name", path.display()))?;
        if name == ".gitkeep" {
            continue;
        }
        if !path.is_file() || path.extension().and_then(|value| value.to_str()) != Some("toml") {
            return Err(format!(
                "{FRAGMENT_DIRECTORY} contains unsupported entry {name}; expected <id>.toml"
            ));
        }
        let identifier = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| format!("{} has an invalid file name", path.display()))?;
        if !valid_identifier(identifier) {
            return Err(format!(
                "{} must use a lowercase dash-separated identifier",
                path.display()
            ));
        }
        let source =
            fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let value: toml::Value = source
            .parse()
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let table = value
            .as_table()
            .ok_or_else(|| format!("{} must contain one TOML table", path.display()))?;
        let mut keys = table.keys().map(String::as_str).collect::<Vec<_>>();
        keys.sort_unstable();
        if keys != ["category", "schema_version", "summary"] {
            return Err(format!(
                "{} must contain exactly schema_version, category, and summary",
                path.display()
            ));
        }
        if text(table, "schema_version")? != SCHEMA_VERSION {
            return Err(format!(
                "{} has an unsupported schema_version",
                path.display()
            ));
        }
        let category = text(table, "category")?;
        if !CATEGORIES.iter().any(|(value, _)| *value == category) {
            return Err(format!(
                "{} has unsupported category {category}",
                path.display()
            ));
        }
        let summary = text(table, "summary")?;
        if summary != summary.trim()
            || summary.is_empty()
            || summary.len() > 512
            || summary.contains(['\n', '\r'])
            || summary.starts_with('-')
        {
            return Err(format!(
                "{} summary must be one trimmed, non-bullet line of at most 512 bytes",
                path.display()
            ));
        }
        fragments.push(Fragment {
            path,
            category: category.into(),
            summary: summary.into(),
        });
    }
    Ok(fragments)
}

fn text<'a>(table: &'a toml::Table, key: &str) -> Result<&'a str> {
    table
        .get(key)
        .and_then(toml::Value::as_str)
        .ok_or_else(|| format!("changelog fragment {key} must be text"))
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && !value.ends_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn render(source: &str, fragments: &[Fragment]) -> Result<String> {
    let marker = "## [Unreleased]";
    let start = source.find(marker).ok_or("CHANGELOG omits Unreleased")?;
    let body_start = start + marker.len();
    let body_end = source[body_start..]
        .find("\n## [")
        .map(|offset| body_start + offset)
        .unwrap_or(source.len());
    let mut unreleased = source[body_start..body_end].to_string();
    let mut grouped = BTreeMap::<&str, Vec<&str>>::new();
    for fragment in fragments {
        grouped
            .entry(fragment.category.as_str())
            .or_default()
            .push(fragment.summary.as_str());
    }
    for (category, heading) in CATEGORIES {
        let Some(summaries) = grouped.get(category) else {
            continue;
        };
        unreleased = insert_summaries(&unreleased, heading, summaries);
    }
    Ok(format!(
        "{}{}{}",
        &source[..body_start],
        unreleased,
        &source[body_end..]
    ))
}

fn insert_summaries(section: &str, heading: &str, summaries: &[&str]) -> String {
    let additions = summaries
        .iter()
        .filter(|summary| !section.lines().any(|line| line == format!("- {summary}")))
        .map(|summary| format!("- {summary}"))
        .collect::<Vec<_>>();
    if additions.is_empty() {
        return section.into();
    }
    let marker = format!("### {heading}");
    if let Some(heading_start) = section.find(&marker) {
        let search_start = heading_start + marker.len();
        let end = section[search_start..]
            .find("\n### ")
            .map(|offset| search_start + offset)
            .unwrap_or(section.len());
        let mut output = String::new();
        output.push_str(section[..end].trim_end());
        output.push_str("\n\n");
        output.push_str(&additions.join("\n"));
        output.push_str("\n\n");
        output.push_str(section[end..].trim_start_matches('\n'));
        output
    } else {
        let mut output = section.trim_end().to_string();
        output.push_str(&format!("\n\n### {heading}\n\n"));
        output.push_str(&additions.join("\n"));
        output.push('\n');
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fragment(category: &str, summary: &str) -> Fragment {
        Fragment {
            path: PathBuf::from("unused.toml"),
            category: category.into(),
            summary: summary.into(),
        }
    }

    #[test]
    fn renders_fragments_into_existing_and_new_categories() {
        let source = "# Changelog\n\n## [Unreleased]\n\n### Added\n\n- Existing.\n\n## [0.1.0]\n";
        let rendered = render(
            source,
            &[
                fragment("added", "Parallel metadata fragments."),
                fragment("fixed", "Conflict-free generated manifest merges."),
            ],
        )
        .unwrap();
        assert!(rendered.contains("- Existing.\n\n- Parallel metadata fragments."));
        assert!(rendered.contains("### Fixed\n\n- Conflict-free generated manifest merges."));
        assert!(rendered.ends_with("## [0.1.0]\n"));
    }

    #[test]
    fn rendering_is_idempotent_for_an_existing_summary() {
        let source = "# Changelog\n\n## [Unreleased]\n\n### Added\n\n- Existing.\n";
        assert_eq!(
            render(source, &[fragment("added", "Existing.")]).unwrap(),
            source
        );
    }

    #[test]
    fn identifiers_are_bounded_to_portable_file_names() {
        assert!(valid_identifier("spi-1001-catalog"));
        assert!(!valid_identifier("SPI-1001"));
        assert!(!valid_identifier("-leading"));
        assert!(!valid_identifier("trailing-"));
        assert!(!valid_identifier("nested/path"));
    }
}
