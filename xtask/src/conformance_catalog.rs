//! The immutable shared catalog closure, reused by accepted and candidate compilation.
use super::*;

pub(super) fn official_catalog_rows(root: &Path) -> TaskResult<Vec<OfficialCatalogRow>> {
    let index_path = root.join("conformance/subsystems/coverage/catalogs/index.json");
    let index = json(&index_path)?;
    let catalogs = indexed_catalog_closure(root, &index, &index_path)?;
    check_catalog_locator_membership(root, &index, &index_path)?;
    let mut rows = Vec::new();
    for (subsystem, catalog_path) in catalogs {
        let catalog = json(&catalog_path)?;
        for unit in array(&catalog, "units", &catalog_path)? {
            let family = text(unit, "id", &catalog_path)?;
            for row in array(unit, "rows", &catalog_path)? {
                rows.push(
                    OfficialCatalogRow::new(
                        text(row, "id", &catalog_path)?,
                        &subsystem,
                        family,
                        text(row, "source_locator", &catalog_path)?,
                        CoverageGate::ALL,
                        ConformanceLimits::default(),
                    )
                    .map_err(|problem| problem.to_string())?,
                );
            }
        }
    }
    require(
        rows.len() == 1_506,
        "shared spec compiler did not load the frozen 1,506-row catalog",
    )?;
    Ok(rows)
}

/// Join catalog locators to their own validated baseline authority. This checks
/// metadata membership only; embedded table/link contents remain body checks,
/// and roadmap dispositions retain all pending execution gates.
pub(super) fn check_catalog_locator_membership(
    root: &Path,
    index: &Value,
    index_path: &Path,
) -> TaskResult {
    topic_manifests::check(root)?;
    for baseline in array(index, "baselines", index_path)? {
        let id = text(baseline, "id", index_path)?;
        let subsystem = text(baseline, "subsystem", index_path)?;
        let (pinned, supporting) =
            topic_manifests::catalog_topic_paths(root, baseline, index_path)?;
        let catalog_path = root.join(text(baseline, "catalog", index_path)?);
        let catalog = json(&catalog_path)?;
        let book = text(&baseline["source"], "book_href", index_path)?;
        for unit in array(&catalog, "units", &catalog_path)? {
            let family = text(unit, "id", &catalog_path)?;
            for row in array(unit, "rows", &catalog_path)? {
                let row_id = text(row, "id", &catalog_path)?;
                let locator = text(row, "source_locator", &catalog_path)?;
                let class = locator.split(':').next().unwrap_or_default();
                if class == "roadmap-normalization" {
                    require(
                        id == "ibm-zos-3.2-dfsms-ams-2026-06"
                            && subsystem == "dataset-vsam-ams"
                            && family == "vsam-primary-organizations"
                            && locator == "roadmap-normalization:vsam-primary-organizations"
                            && (1..=5).any(|number| row_id == format!("{id}:{family}:{number:04}")),
                        &format!("official row {row_id} has no declared zero-credit normalization"),
                    )?;
                    continue;
                }
                let topic = match (class, subsystem) {
                    ("topic", _) => topic_manifests::locator_topic_path(locator),
                    ("html-table", "cics" | "ims") | ("html-link", "mq") => {
                        pinned.contains(book).then_some(book)
                    }
                    ("html-table", "racf-saf") if supporting.len() == 1 => {
                        supporting.iter().next().map(String::as_str)
                    }
                    _ => None,
                };
                require(
                    topic.is_some_and(|path| pinned.contains(path) || supporting.contains(path)),
                    &format!(
                        "official row {row_id} locator is outside baseline {id} topic authority \
                         or has an unsupported class: {locator}"
                    ),
                )?;
            }
        }
    }
    Ok(())
}

pub(super) fn indexed_catalog_closure(
    root: &Path,
    index: &Value,
    index_path: &Path,
) -> TaskResult<Vec<(String, PathBuf)>> {
    let mut catalogs = Vec::new();
    for baseline in array(index, "baselines", index_path)? {
        let subsystem = text(baseline, "subsystem", index_path)?.to_string();
        let catalog_relative = text(baseline, "catalog", index_path)?;
        require(
            catalog_relative.starts_with("conformance/subsystems/coverage/catalogs/")
                && catalog_relative.ends_with(".json")
                && !catalog_relative.contains(".."),
            &format!("indexed catalog path is unsafe: {catalog_relative}"),
        )?;
        let catalog_path = root.join(catalog_relative);
        let expected = text(baseline, "catalog_sha256", index_path)?;
        validate_sha256_identity(expected, "indexed catalog digest")?;
        let actual = format!("sha256:{}", file_digest(&catalog_path)?);
        require(
            actual == expected,
            &format!("indexed catalog digest drifted: {catalog_relative}"),
        )?;
        catalogs.push((subsystem, catalog_path));
    }
    Ok(catalogs)
}

#[cfg(test)]
mod locator_membership_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    const INDEX: &str = "conformance/subsystems/coverage/catalogs/index.json";
    const ROW: &str = "ibm-zos-3.2-dfsms-ams-2026-06:ams-functional-commands:0001";

    struct Fixture {
        root: PathBuf,
        original_index: Value,
    }

    impl Fixture {
        fn new() -> Self {
            let source = repository_root().expect("current checkout");
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = env::temp_dir().join(format!(
                "cv-201-locator-membership-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&root).unwrap();
            let fixture = Self {
                root,
                original_index: json(&source.join(INDEX)).unwrap(),
            };
            // Copy only metadata required by the real offline coverage gate, never topic bodies.
            for directory in [
                "conformance/subsystems/coverage/catalogs",
                "conformance/subsystems/coverage/manifests",
                "conformance/subsystems/cics/application/manifests",
                "conformance/subsystems/cics/system/manifests",
                "conformance/subsystems/ims/manifests",
                "conformance/subsystems/mq/manifests",
            ] {
                for entry in fs::read_dir(source.join(directory)).unwrap() {
                    let path = entry.unwrap().path();
                    if path.extension().and_then(OsStr::to_str) == Some("json") {
                        fixture.copy(&source, path.strip_prefix(&source).unwrap());
                    }
                }
            }
            for schema in [
                "conformance/subsystems/coverage/schemas/topic-manifest.schema.json",
                "conformance/subsystems/cics/application/schemas/topic-manifest-registry.schema.json",
            ] {
                fixture.copy(&source, Path::new(schema));
            }
            fixture.assert_metadata_coherent();
            fixture
        }

        fn copy(&self, source: &Path, relative: &Path) {
            let destination = self.root.join(relative);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::copy(source.join(relative), destination).unwrap();
        }

        fn baseline(&self, subsystem: &str) -> Value {
            json(&self.root.join(INDEX)).unwrap()["baselines"]
                .as_array()
                .unwrap()
                .iter()
                .find(|baseline| baseline["subsystem"] == subsystem)
                .unwrap()
                .clone()
        }

        fn pinned_topics(&self, baseline: &Value) -> BTreeSet<String> {
            topic_manifests::pinned_topic_paths(
                &self.root,
                baseline["source"]["manifest"].as_str().unwrap(),
                baseline["id"].as_str().unwrap(),
                baseline["source"]["sha256"].as_str().unwrap(),
            )
            .unwrap()
        }

        fn catalog(&self, subsystem: &str) -> Value {
            let baseline = self.baseline(subsystem);
            json(&self.root.join(baseline["catalog"].as_str().unwrap())).unwrap()
        }

        fn replace_topic(&self, replacement: &str) {
            let baseline = self.baseline("dataset-vsam-ams");
            let path = self.root.join(baseline["catalog"].as_str().unwrap());
            let mut catalog = json(&path).unwrap();
            let row = &mut catalog["units"][0]["rows"][0];
            assert_eq!(row["id"], ROW);
            let old_locator = row["source_locator"].as_str().unwrap();
            let old_path = topic_manifests::locator_topic_path(old_locator).unwrap();
            assert!(self.pinned_topics(&baseline).contains(old_path));
            row["source_locator"] = Value::String(old_locator.replacen(old_path, replacement, 1));
            self.write_catalog_and_fixture_digest(&baseline, &catalog);
        }

        fn replace_locator(&self, replacement: &str) {
            let baseline = self.baseline("dataset-vsam-ams");
            let mut catalog = self.catalog("dataset-vsam-ams");
            assert_eq!(catalog["units"][0]["rows"][0]["id"], ROW);
            catalog["units"][0]["rows"][0]["source_locator"] = json!(replacement);
            self.write_catalog_and_fixture_digest(&baseline, &catalog);
        }

        fn write_catalog_and_fixture_digest(&self, baseline: &Value, catalog: &Value) {
            let path = self.root.join(baseline["catalog"].as_str().unwrap());
            fs::write(&path, serde_json::to_vec_pretty(catalog).unwrap()).unwrap();
            let index_path = self.root.join(INDEX);
            let mut index = json(&index_path).unwrap();
            let changed = index["baselines"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|entry| entry["id"] == baseline["id"])
                .unwrap();
            // Only the temporary catalog digest changes; committed pins never change.
            changed["catalog_sha256"] = json!(format!("sha256:{}", file_digest(&path).unwrap()));
            fs::write(index_path, serde_json::to_vec_pretty(&index).unwrap()).unwrap();
            self.assert_metadata_coherent();
        }

        fn assert_metadata_coherent(&self) {
            let index = json(&self.root.join(INDEX)).unwrap();
            assert_eq!(index["mandatory_rows"], 1506);
            assert_eq!(index["baseline_count"], 9);
            let current = index["baselines"].as_array().unwrap();
            let original = self.original_index["baselines"].as_array().unwrap();
            assert_eq!(current.len(), 9);
            for (current, original) in current.iter().zip(original) {
                for field in [
                    "id",
                    "subsystem",
                    "source",
                    "supporting_sources",
                    "immutable_denominators",
                    "mandatory_rows",
                ] {
                    assert_eq!(current[field], original[field], "fixture changed {field}");
                }
                let path = self.root.join(current["catalog"].as_str().unwrap());
                assert_eq!(
                    current["catalog_sha256"],
                    format!("sha256:{}", file_digest(&path).unwrap())
                );
            }
            // The production manifest validator, not a test-side hash implementation.
            topic_manifests::check(&self.root).expect("unchanged valid manifest closure");
        }

        fn assert_allowed(&self) -> Vec<OfficialCatalogRow> {
            crate::check_coverage(&self.root).expect("real coverage gate accepts control");
            let rows =
                official_catalog_rows(&self.root).expect("real typed reader accepts control");
            assert_eq!(rows.len(), 1506);
            rows
        }

        fn assert_both_gates_reject(&self, row_id: &str) {
            let coverage = crate::check_coverage(&self.root);
            let reader = official_catalog_rows(&self.root);
            assert!(
                coverage.is_err() && reader.is_err(),
                "coherent locator mutant accepted: row={row_id}; coverage={coverage:?}; \
                 typed_reader={:?}",
                reader.as_ref().map(Vec::len)
            );
            for error in [coverage.unwrap_err(), reader.unwrap_err()] {
                assert!(
                    error.contains(row_id),
                    "refusal must identify its catalog row: {error}"
                );
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn locator_membership_rejects_rehashed_non_cobol_unlisted_topic() {
        let fixture = Fixture::new();
        fixture.assert_allowed();
        let replacement = "SSLTBW_3.2.0/cv201-fixture/unlisted.htm";
        assert!(
            !fixture
                .pinned_topics(&fixture.baseline("dataset-vsam-ams"))
                .contains(replacement)
        );
        fixture.replace_topic(replacement);
        fixture.assert_both_gates_reject(ROW);
    }

    #[test]
    fn locator_membership_rejects_topic_pinned_to_a_different_product() {
        let fixture = Fixture::new();
        fixture.assert_allowed();
        let foreign = fixture.baseline("mq");
        let replacement = foreign["source"]["book_href"].as_str().unwrap();
        assert!(fixture.pinned_topics(&foreign).contains(replacement));
        assert!(
            !fixture
                .pinned_topics(&fixture.baseline("dataset-vsam-ams"))
                .contains(replacement)
        );
        fixture.replace_topic(replacement);
        fixture.assert_both_gates_reject(ROW);
    }

    #[test]
    fn locator_membership_rejects_topic_from_another_baseline_of_the_same_product() {
        let fixture = Fixture::new();
        fixture.assert_allowed();
        let foreign = fixture.baseline("racf-saf");
        let own = fixture.baseline("dataset-vsam-ams");
        assert_eq!(foreign["source"]["product"], own["source"]["product"]);
        let foreign_pins = fixture.pinned_topics(&foreign);
        let own_pins = fixture.pinned_topics(&own);
        let replacement = foreign_pins.difference(&own_pins).next().unwrap();
        fixture.replace_topic(replacement);
        fixture.assert_both_gates_reject(ROW);
    }

    #[test]
    fn locator_membership_rejects_unknown_locator_class() {
        let fixture = Fixture::new();
        fixture.assert_allowed();
        fixture.replace_locator("unsupported-source-class:cv201-fixture");
        fixture.assert_both_gates_reject(ROW);
    }

    #[test]
    fn locator_membership_normalization_cannot_hide_an_ordinary_command_row() {
        let fixture = Fixture::new();
        fixture.assert_allowed();
        fixture.replace_locator("roadmap-normalization:vsam-primary-organizations");
        fixture.assert_both_gates_reject(ROW);
    }

    #[test]
    fn locator_membership_accepts_unchanged_pinned_topic_and_embedded_links() {
        let fixture = Fixture::new();
        let rows = fixture.assert_allowed();
        assert!(rows.iter().any(|row| row.row_id().as_str() == ROW));
        let mq = fixture.baseline("mq");
        assert!(
            fixture
                .pinned_topics(&mq)
                .contains(mq["source"]["book_href"].as_str().unwrap())
        );
        let links = rows
            .iter()
            .filter(|row| row.subsystem() == "mq")
            .collect::<Vec<_>>();
        assert_eq!(links.len(), 26);
        assert!(
            links
                .iter()
                .all(|row| row.source_locator().starts_with("html-link:"))
        );
    }

    #[test]
    fn locator_membership_accepts_racroute_supporting_receipt_and_implicit_tables() {
        let fixture = Fixture::new();
        let racf = fixture.baseline("racf-saf");
        let supporting = racf["supporting_sources"].as_array().unwrap();
        assert_eq!(supporting.len(), 1);
        let path = supporting[0]["topic_path"].as_str().unwrap();
        // This control proves membership can come from a receipt outside the primary manifest.
        assert!(!fixture.pinned_topics(&racf).contains(path));
        assert_eq!(supporting[0]["product"], racf["source"]["product"]);
        crate::validate_official_source(
            &supporting[0],
            &fixture.root.join(INDEX),
            racf["id"].as_str().unwrap(),
        )
        .unwrap();
        let rows = fixture.assert_allowed();
        for (subsystem, expected) in [("cics", 571), ("ims", 25), ("racf-saf", 14)] {
            assert_eq!(
                rows.iter()
                    .filter(|row| row.subsystem() == subsystem
                        && row.source_locator().starts_with("html-table:"))
                    .count(),
                expected
            );
        }
    }

    #[test]
    fn locator_membership_preserves_five_zero_credit_normalization_rows() {
        let fixture = Fixture::new();
        let rows = fixture.assert_allowed();
        let normalized = rows
            .iter()
            .filter(|row| {
                row.source_locator() == "roadmap-normalization:vsam-primary-organizations"
            })
            .collect::<Vec<_>>();
        assert_eq!(normalized.len(), 5);
        let baseline = fixture.baseline("dataset-vsam-ams");
        let manifest = json(
            &fixture
                .root
                .join(baseline["source"]["manifest"].as_str().unwrap()),
        )
        .unwrap();
        assert_eq!(manifest["coverage_credit"], 0);
        for row in normalized {
            assert_eq!(row.subsystem(), "dataset-vsam-ams");
            assert_eq!(row.family(), "vsam-primary-organizations");
            assert_eq!(row.applicable_gates(), &BTreeSet::from(CoverageGate::ALL));
        }
        assert_eq!(
            fixture.original_index["claim_policy"]["catalog_presence_counts_as_execution"],
            false
        );
    }
}
