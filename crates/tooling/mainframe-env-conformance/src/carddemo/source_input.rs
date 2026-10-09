use super::*;

pub(super) fn collect_paths(
    corpus_dir: &Path,
    roots: &[&str],
    extension: &str,
) -> Result<Vec<String>, CorpusProblem> {
    let mut paths = Vec::new();
    for root in roots {
        let directory = corpus_dir.join(root);
        let entries = fs::read_dir(&directory).map_err(|error| {
            CorpusProblem::new(
                "carddemo.source.file_missing",
                format!("cannot enumerate source root {root}: {error}"),
            )
        })?;
        for entry in entries {
            let path = entry
                .map_err(|error| {
                    CorpusProblem::new(
                        "carddemo.source.file_missing",
                        format!("cannot enumerate source root {root}: {error}"),
                    )
                })?
                .path();
            if !path.is_file()
                || !path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case(extension))
            {
                continue;
            }
            let relative = path
                .strip_prefix(corpus_dir)
                .ok()
                .and_then(Path::to_str)
                .ok_or_else(|| {
                    CorpusProblem::new(
                        "carddemo.source.path_invalid",
                        "source path is not a repository-relative UTF-8 path",
                    )
                })?;
            paths.push(relative.to_string());
        }
    }
    paths.sort();
    Ok(paths)
}

pub(super) fn source_file(
    corpus_dir: &Path,
    relative: &str,
    limits: SourceLimits,
) -> Result<SourceFile, CorpusProblem> {
    validate_relative_path(relative, "source file")?;
    let bytes = read_corpus_file(corpus_dir, &corpus_dir.join(relative))?;
    SourceFile::input(
        relative,
        bytes,
        SourceFormat::Fixed,
        SourceEncoding::Utf8,
        limits,
    )
    .map_err(|error| {
        CorpusProblem::new(
            "carddemo.source.bundle_invalid",
            format!("cannot load source file {relative}: {error}"),
        )
    })
}

/// The focused transaction fixture selects its complete closure before compilation.
pub(super) fn transaction_bundles(
    corpus_dir: &Path,
) -> Result<Vec<(String, SourceBundle)>, CorpusProblem> {
    selected_transaction_bundles(
        corpus_dir,
        &[
            "app/cbl/COSGN00C.cbl",
            "app/cbl/COMEN01C.cbl",
            "app/cbl/COTRN02C.cbl",
            "app/cbl/CSUTLDTC.cbl",
        ],
        &[
            "app/cpy-bms/COSGN00.CPY",
            "app/cpy-bms/COMEN01.CPY",
            "app/cpy-bms/COTRN02.CPY",
        ],
    )
}

pub(super) fn navigation_bundles(
    corpus_dir: &Path,
) -> Result<Vec<(String, SourceBundle)>, CorpusProblem> {
    selected_transaction_bundles(
        corpus_dir,
        &[
            "app/cbl/COSGN00C.cbl",
            "app/cbl/COMEN01C.cbl",
            "app/cbl/COTRN00C.cbl",
            "app/cbl/COTRN01C.cbl",
            "app/cbl/COTRN02C.cbl",
            "app/cbl/CSUTLDTC.cbl",
        ],
        &[
            "app/cpy-bms/COSGN00.CPY",
            "app/cpy-bms/COMEN01.CPY",
            "app/cpy-bms/COTRN00.CPY",
            "app/cpy-bms/COTRN01.CPY",
            "app/cpy-bms/COTRN02.CPY",
        ],
    )
}

fn selected_transaction_bundles(
    corpus_dir: &Path,
    primary_paths: &[&str],
    map_paths: &[&str],
) -> Result<Vec<(String, SourceBundle)>, CorpusProblem> {
    let limits = SourceLimits::default();
    let application_paths = [
        "app/cpy/COCOM01Y.cpy",
        "app/cpy/COTTL01Y.cpy",
        "app/cpy/CSDAT01Y.cpy",
        "app/cpy/CSMSG01Y.cpy",
        "app/cpy/CSUSR01Y.cpy",
        "app/cpy/COMEN02Y.cpy",
        "app/cpy/CVTRA05Y.cpy",
        "app/cpy/CVACT01Y.cpy",
        "app/cpy/CVACT03Y.cpy",
    ];
    let mut copybooks = Vec::new();
    let mut libraries = Vec::new();
    for (index, paths) in [application_paths.as_slice(), map_paths]
        .into_iter()
        .enumerate()
    {
        let members = paths
            .iter()
            .map(|path| LogicalPath::new(*path, limits.max_path_bytes))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                CorpusProblem::new("carddemo.layout.closure_invalid", error.to_string())
            })?;
        libraries.push(
            SourceLibrary::new(format!("application-{index:02}"), members, limits).map_err(
                |error| CorpusProblem::new("carddemo.layout.closure_invalid", error.to_string()),
            )?,
        );
        for path in paths {
            copybooks.push(source_file(corpus_dir, path, limits)?);
        }
    }
    let abi = materialize_host_abi_libraries(&[cics_abi_library()], limits)
        .map_err(|error| CorpusProblem::new("carddemo.abi.invalid", error.to_string()))?;
    copybooks.extend(abi.files);
    libraries.extend(abi.libraries);
    primary_paths
        .iter()
        .copied()
        .map(|path| {
            let mut files = vec![source_file(corpus_dir, path, limits)?];
            files.extend(copybooks.iter().cloned());
            let logical = LogicalPath::new(path, limits.max_path_bytes).map_err(|error| {
                CorpusProblem::new("carddemo.layout.closure_invalid", error.to_string())
            })?;
            let bundle = SourceBundle::with_libraries(
                &logical,
                files,
                libraries.clone(),
                BTreeMap::new(),
                Vec::new(),
                limits,
            )
            .map_err(|error| {
                CorpusProblem::new("carddemo.layout.closure_invalid", error.to_string())
            })?;
            Ok((path.into(), bundle))
        })
        .collect()
}

pub(super) fn subsystem_abi_libraries(
    limits: SourceLimits,
) -> Result<MaterializedHostAbiLibraries, CorpusProblem> {
    materialize_host_abi_libraries(&subsystem_abi_definitions(), limits).map_err(|error| {
        CorpusProblem::new(
            "carddemo.abi.invalid",
            format!("subsystem ABI source libraries are invalid: {error}"),
        )
    })
}
