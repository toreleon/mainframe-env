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
