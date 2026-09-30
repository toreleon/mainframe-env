//! Every committed topic manifest, checked against the explicit pin that names it.
//!
//! `conformance/0.2/catalogs/index.json` stopped pinning a file's bytes when the
//! baselines moved onto documentation topics: a `documentation-topics` source
//! pins `topic_manifest_digest`, a digest over the whole book's topic list, and
//! names the manifest that list lives in. Nothing in Rust recomputed it, so a
//! corrupted topic `sha256` inside a manifest left the recorded digest no longer
//! following from its own rows and the receipt no longer describing the manifest,
//! while every gate reported pass. The only check was a Python test CI does not
//! discover.
//!
//! This is deliberately offline. It hashes committed metadata and never asks IBM
//! anything; re-reading the topics is `conformance/tools/fetch_pinned_sources.py`,
//! which is a review activity and cannot run in CI.

use super::*;

const INDEX_PATH: &str = "conformance/0.2/catalogs/index.json";
const MANIFEST_DIRECTORY: &str = "conformance/0.2/manifests";
const MANIFEST_PREFIX: &str = "conformance/0.2/manifests/";
struct LaterRegistry {
    registry_path: &'static str,
    manifest_directory: &'static str,
    manifest_prefix: &'static str,
    target_version: &'static str,
}

const LATER_REGISTRIES: &[LaterRegistry] = &[
    LaterRegistry {
        registry_path: "conformance/0.9/manifests/index.json",
        manifest_directory: "conformance/0.9/manifests",
        manifest_prefix: "conformance/0.9/manifests/",
        target_version: "0.9.0",
    },
    LaterRegistry {
        registry_path: "conformance/0.14/manifests/index.json",
        manifest_directory: "conformance/0.14/manifests",
        manifest_prefix: "conformance/0.14/manifests/",
        target_version: "0.14.0",
    },
];

/// The one definition of `topic_manifest_digest`, stated the same way the
/// manifest schema states it as a `const` and `conformance/tools/docs_api.py`
/// implements it. Every manifest repeats the sentence, and this check requires
/// the sentence and the arithmetic below to agree, so the definition cannot be
/// quietly reinterpreted on one side.
const DIGEST_DEFINITION: &str = "sha256 over the concatenation, sorted by topic_path, of one \
     \"<topic_path> <sha256>\\n\" line per topic, each <sha256> written as bare lowercase hex";

pub(super) fn check(root: &Path) -> TaskResult {
    let index_path = root.join(INDEX_PATH);
    let index = json(&index_path)?;
    let mut pinned = BTreeSet::new();
    for baseline in array(&index, "baselines", &index_path)? {
        let id = text(baseline, "id", &index_path)?;
        let subsystem = text(baseline, "subsystem", &index_path)?;
        let source = &baseline["source"];
        require(
            text(source, "kind", &index_path)? == "documentation-topics",
            &format!("baseline {id} is not read from documentation topics"),
        )?;
        let relative = text(source, "manifest", &index_path)?;
        require(
            pinned.insert(relative.to_string()),
            &format!("baseline {id} pins a topic manifest another baseline already pins"),
        )?;
        let manifest_path = manifest_path(root, relative, id)?;
        let manifest = json(&manifest_path)?;
        let digest = recompute(&manifest, &manifest_path)?;
        require(
            text(&manifest, "topic_manifest_digest", &manifest_path)? == digest,
            &format!("{relative} records a topic manifest digest its own topics do not produce"),
        )?;
        require(
            text(source, "sha256", &index_path)? == format!("sha256:{digest}"),
            &format!("baseline {id} pins a digest {relative} no longer produces"),
        )?;

        // The receipt and the manifest describe one retrieval, so every field
        // they both carry must agree; otherwise the pin describes a book that is
        // not the one the manifest lists.
        require(
            text(&manifest, "schema_version", &manifest_path)? == "mainframe-env.topic-manifest@1"
                && text(&manifest, "target_version", &manifest_path)? == "0.2.0"
                && text(&manifest, "baseline_id", &manifest_path)? == id
                && text(&manifest, "subsystem", &manifest_path)? == subsystem,
            &format!("{relative} identity differs from baseline {id}"),
        )?;
        for field in [
            "product",
            "book_href",
            "snapshot_date",
            "content_url_template",
        ] {
            require(
                text(&manifest, field, &manifest_path)? == text(source, field, &index_path)?,
                &format!("{relative} and baseline {id} disagree about {field}"),
            )?;
        }
        require(
            text(&manifest, "toc_url", &manifest_path)? == text(source, "url", &index_path)?,
            &format!("{relative} and baseline {id} disagree about the table-of-contents URL"),
        )?;
        require(
            format!("sha256:{}", text(&manifest, "toc_sha256", &manifest_path)?)
                == text(source, "toc_sha256", &index_path)?,
            &format!("{relative} and baseline {id} disagree about the table-of-contents digest"),
        )?;
        let topics = array(&manifest, "topics", &manifest_path)?;
        require(
            manifest["topic_count"].as_u64() == Some(topics.len() as u64)
                && source["topic_count"].as_u64() == Some(topics.len() as u64),
            &format!("{relative} topic count is not the number of topics it lists"),
        )?;
        let mut total = 0_u64;
        for topic in topics {
            total += topic["bytes"]
                .as_u64()
                .ok_or_else(|| format!("{relative} has a topic with no byte count"))?;
        }
        require(
            manifest["total_bytes"].as_u64() == Some(total)
                && source["bytes"].as_u64() == Some(total),
            &format!("{relative} total byte count is not the sum of its topics"),
        )?;
        require(
            manifest["coverage_credit"].as_u64() == Some(0)
                && manifest["retained_in_repository"] == Value::Bool(false),
            &format!("{relative} claims coverage credit or retained publication bytes"),
        )?;
    }

    // A manifest nothing pins is a publication-derived artifact with no receipt,
    // which is exactly what the 0.2 tree is not allowed to accumulate.
    let directory = root.join(MANIFEST_DIRECTORY);
    let mut present = BTreeSet::new();
    for entry in
        fs::read_dir(&directory).map_err(|error| format!("{}: {error}", directory.display()))?
    {
        let path = entry.map_err(|error| error.to_string())?.path();
        let name = path
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or_else(|| format!("{} has an unreadable name", path.display()))?;
        present.insert(format!("{MANIFEST_PREFIX}{name}"));
    }
    require(
        present == pinned,
        "conformance/0.2/manifests holds a manifest no baseline pins, or is missing one",
    )?;
    for registry in LATER_REGISTRIES {
        check_later_registry(root, registry)?;
    }
    Ok(())
}

fn check_later_registry(root: &Path, config: &LaterRegistry) -> TaskResult {
    let registry_path = root.join(config.registry_path);
    let registry = json(&registry_path)?;
    let registry_schema = root.join("conformance/0.9/schemas/topic-manifest-registry.schema.json");
    validate_schema_instance(&json(&registry_schema)?, &registry, &registry_path)?;
    require(
        text(&registry, "target_version", &registry_path)? == config.target_version,
        &format!(
            "{} does not own target version {}",
            config.registry_path, config.target_version
        ),
    )?;
    let manifest_schema = root.join("conformance/0.2/schemas/topic-manifest.schema.json");
    let mut pinned = BTreeSet::new();
    let mut scopes = BTreeSet::new();
    for entry in array(&registry, "manifests", &registry_path)? {
        let scope = text(entry, "scope_id", &registry_path)?;
        require(
            scopes.insert(scope.to_string()),
            &format!("later topic manifest scope {scope} is repeated"),
        )?;
        let relative = text(entry, "manifest", &registry_path)?;
        require(
            pinned.insert(relative.to_string()),
            &format!("later topic manifest {relative} is registered twice"),
        )?;
        let path = later_manifest_path(root, relative, scope, config)?;
        let manifest_sha256 = format!(
            "sha256:{:x}",
            Sha256::digest(
                fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?
            )
        );
        let manifest = json(&path)?;
        validate_schema_instance(&json(&manifest_schema)?, &manifest, &path)?;
        let digest = recompute(&manifest, &path)?;
        require(
            text(&manifest, "schema_version", &path)? == "mainframe-env.topic-manifest@1"
                && text(&manifest, "target_version", &path)? == config.target_version
                && text(&manifest, "baseline_id", &path)?
                    == text(entry, "baseline_id", &registry_path)?
                && text(&manifest, "subsystem", &path)?
                    == text(entry, "subsystem", &registry_path)?
                && text(&manifest, "topic_manifest_digest", &path)? == digest
                && text(entry, "manifest_sha256", &registry_path)? == manifest_sha256
                && text(entry, "topic_manifest_sha256", &registry_path)?
                    == format!("sha256:{digest}")
                && entry["topic_count"].as_u64()
                    == Some(array(&manifest, "topics", &path)?.len() as u64)
                && entry["coverage_credit"].as_u64() == Some(0)
                && entry["semantic_authority"] == Value::Bool(false)
                && manifest["coverage_credit"].as_u64() == Some(0)
                && manifest["retained_in_repository"] == Value::Bool(false),
            &format!("later topic manifest {relative} disagrees with scope {scope}"),
        )?;
    }
    let directory = root.join(config.manifest_directory);
    let mut present = BTreeSet::new();
    for entry in
        fs::read_dir(&directory).map_err(|error| format!("{}: {error}", directory.display()))?
    {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.file_name() == Some(OsStr::new("index.json")) {
            continue;
        }
        let name = path
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or_else(|| format!("{} has an unreadable name", path.display()))?;
        present.insert(format!("{}{name}", config.manifest_prefix));
    }
    require(
        present == pinned,
        &format!(
            "{} holds an unregistered manifest or is missing one",
            config.manifest_directory
        ),
    )
}

/// The topic paths one pinned manifest lists, after proving the manifest is the
/// one the caller's digest names.
///
/// A locator that cites a topic outside the pinned manifest is citing something
/// this repository never read, and until this existed the 28 COBOL
/// special-register locators had nothing but a schema regex between them and a
/// fabricated topic path.
pub(super) fn pinned_topic_paths(
    root: &Path,
    relative: &str,
    owner: &str,
    expected_digest: &str,
) -> TaskResult<BTreeSet<String>> {
    let manifest_path = manifest_path(root, relative, owner)?;
    let manifest = json(&manifest_path)?;
    let digest = recompute(&manifest, &manifest_path)?;
    require(
        text(&manifest, "topic_manifest_digest", &manifest_path)? == digest,
        &format!("{relative} records a topic manifest digest its own topics do not produce"),
    )?;
    require(
        expected_digest == digest || expected_digest == format!("sha256:{digest}"),
        &format!("{owner} pins a digest {relative} no longer produces"),
    )?;
    Ok(array(&manifest, "topics", &manifest_path)?
        .iter()
        .filter_map(|topic| topic["topic_path"].as_str().map(str::to_string))
        .collect())
}

/// The topic path component of a `topic:...;topic-id:...;heading:...` locator.
///
/// A heading may contain a semicolon -- Db2 heads a topic `DECLARE; THEN` -- so
/// only the first component is delimited by one, which is why the locator puts
/// the path first.
pub(super) fn locator_topic_path(locator: &str) -> Option<&str> {
    let rest = locator.strip_prefix("topic:")?;
    let path = rest.split(';').next()?;
    (!path.is_empty()).then_some(path)
}

fn manifest_path(root: &Path, relative: &str, owner: &str) -> TaskResult<PathBuf> {
    require(
        relative.starts_with(MANIFEST_PREFIX)
            && relative.ends_with(".json")
            && !relative.contains("..")
            && relative[MANIFEST_PREFIX.len()..].find('/').is_none(),
        &format!("{owner} names an unsafe topic manifest path {relative}"),
    )?;
    Ok(root.join(relative))
}

fn later_manifest_path(
    root: &Path,
    relative: &str,
    owner: &str,
    config: &LaterRegistry,
) -> TaskResult<PathBuf> {
    require(
        relative.starts_with(config.manifest_prefix)
            && relative.ends_with(".json")
            && !relative.contains("..")
            && relative[config.manifest_prefix.len()..].find('/').is_none()
            && relative != config.registry_path,
        &format!("{owner} names an unsafe later topic manifest path {relative}"),
    )?;
    Ok(root.join(relative))
}

/// `topic_manifest_digest`, recomputed from the manifest's own topic list.
fn recompute(manifest: &Value, path: &Path) -> TaskResult<String> {
    require(
        text(manifest, "topic_manifest_digest_definition", path)? == DIGEST_DEFINITION,
        &format!(
            "{} states a topic manifest digest definition xtask does not implement",
            path.display()
        ),
    )?;
    let product = text(manifest, "product", path)?;
    let mut lines = BTreeSet::new();
    for topic in array(manifest, "topics", path)? {
        let topic_path = text(topic, "topic_path", path)?;
        require(
            topic_path.starts_with(&format!("{product}/")),
            &format!(
                "{} lists topic {topic_path} from another product",
                path.display()
            ),
        )?;
        let digest = text(topic, "sha256", path)?;
        require(
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
            &format!(
                "{} records topic {topic_path} without a bare lowercase SHA-256",
                path.display()
            ),
        )?;
        require(
            topic["bytes"].as_u64().is_some_and(|bytes| bytes > 0)
                && text(topic, "last_modified", path)?.len() >= 4,
            &format!(
                "{} records topic {topic_path} without provenance",
                path.display()
            ),
        )?;
        require(
            lines.insert(format!("{topic_path} {digest}\n")),
            &format!("{} lists topic {topic_path} twice", path.display()),
        )?;
    }
    require(
        !lines.is_empty(),
        &format!("{} lists no topics", path.display()),
    )?;
    Ok(format!(
        "{:x}",
        Sha256::digest(lines.into_iter().collect::<String>())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(topics: &[(&str, &str)]) -> Value {
        json!({
            "product": "p",
            "topic_manifest_digest_definition": DIGEST_DEFINITION,
            "topics": topics
                .iter()
                .map(|(path, digest)| json!({
                    "topic_path": path,
                    "sha256": digest,
                    "bytes": 1,
                    "last_modified": "2026-01-01",
                }))
                .collect::<Vec<_>>(),
        })
    }

    /// The digest definition the schema states as a `const`, worked by hand.
    /// `conformance/tools/tests/test_locator_tools.py` pins the same shape for
    /// the Python side; if the two ever disagree, the pins mean two things.
    #[test]
    fn the_digest_is_taken_over_sorted_topic_lines() {
        let unsorted = manifest(&[("p/b.html", &"b".repeat(64)), ("p/a.html", &"a".repeat(64))]);
        assert_eq!(
            recompute(&unsorted, Path::new("in-memory")).expect("digest"),
            "b9dfe562edfdca05d4ccc64c072f5f980918778feea896838d39fb62239ad5e4"
        );
        let flipped = manifest(&[
            ("p/b.html", &"b".repeat(64)),
            ("p/a.html", &format!("c{}", "a".repeat(63))),
        ]);
        assert_ne!(
            recompute(&flipped, Path::new("in-memory")).expect("digest"),
            recompute(&unsorted, Path::new("in-memory")).expect("digest"),
            "one flipped hex digit must move the digest"
        );
    }

    #[test]
    fn a_topic_from_another_product_or_a_repeated_one_is_refused() {
        let foreign = manifest(&[("q/a.html", &"a".repeat(64))]);
        assert!(recompute(&foreign, Path::new("in-memory")).is_err());
        let repeated = manifest(&[("p/a.html", &"a".repeat(64)), ("p/a.html", &"a".repeat(64))]);
        assert!(recompute(&repeated, Path::new("in-memory")).is_err());
        let uppercase = manifest(&[("p/a.html", &"A".repeat(64))]);
        assert!(recompute(&uppercase, Path::new("in-memory")).is_err());
    }

    /// A heading may carry a semicolon; the topic path may not, which is why it
    /// comes first.
    #[test]
    fn only_the_first_locator_component_is_the_topic_path() {
        assert_eq!(
            locator_topic_path("topic:a/b.html;topic-id:x;heading:DECLARE; THEN"),
            Some("a/b.html")
        );
        assert_eq!(locator_topic_path("pdf-page:19;outline:ADDRESS OF"), None);
        assert_eq!(locator_topic_path("topic:;topic-id:x;heading:y"), None);
    }

    #[test]
    fn every_committed_manifest_reproduces_the_digest_its_receipt_pins() {
        check(&repository_root().expect("repository root")).expect("topic manifests");
    }

    /// Every locator in `conformance/0.3/cobol/language.json` -- the 173 official
    /// rows and the 28 special registers alike -- names a topic the COBOL
    /// baseline actually read.
    #[test]
    fn the_cobol_catalog_cites_only_pinned_topics() {
        let root = repository_root().expect("repository root");
        let path = root.join("conformance/0.3/cobol/language.json");
        crate::check_cobol_language_catalog(&root, &path).expect("COBOL language catalog");
    }
}
