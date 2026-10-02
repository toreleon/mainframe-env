//! The immutable shared catalog closure, reused by accepted and candidate compilation.
use super::*;

pub(super) fn official_catalog_rows(root: &Path) -> TaskResult<Vec<OfficialCatalogRow>> {
    let index_path = root.join("conformance/0.2/catalogs/index.json");
    let index = json(&index_path)?;
    let catalogs = indexed_catalog_closure(root, &index, &index_path)?;
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
            catalog_relative.starts_with("conformance/0.2/catalogs/")
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
