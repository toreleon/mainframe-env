//! Source-backed base online application composition.
use super::*;

pub(super) fn carddemo_base_online_definition(
    corpus_dir: &Path,
) -> Result<OnlineApplicationDefinition, CorpusProblem> {
    let mut bundles = BTreeMap::new();
    for (primary, bundle) in explicit_carddemo_bundles(corpus_dir)? {
        if !primary.starts_with("app/cbl/") {
            continue;
        }
        let name = Path::new(&primary)
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                CorpusProblem::new("carddemo.online.program_invalid", "program name is invalid")
            })?
            .to_ascii_uppercase();
        if bundles.insert(name.clone(), (primary, bundle)).is_some() {
            return Err(CorpusProblem::new(
                "carddemo.online.program_duplicate",
                format!("{name} is duplicated"),
            ));
        }
    }
    let csd = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/csd/CARDDEMO.CSD"),
    )?)
    .map_err(|_| CorpusProblem::new("carddemo.online.csd_invalid", "base CSD is not UTF-8"))?;
    let resources = parse_csd(&csd).map_err(package_problem)?;
    let mut transactions = BTreeMap::new();
    for resource in resources
        .into_iter()
        .filter(|resource| resource.kind == "TRANSACTION")
    {
        let program = resource.properties.get("PROGRAM").ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.online.csd_invalid",
                format!("{} program is missing", resource.name),
            )
        })?;
        if bundles.contains_key(program) {
            transactions.insert(resource.name, program.clone());
        }
    }
    let mut needed = transactions.values().cloned().collect::<BTreeSet<_>>();
    needed.insert("CSUTLDTC".into());
    let compiler = CobolCompiler::default();
    let mut programs = Vec::new();
    let mut semantic_models = Vec::new();
    for name in &needed {
        let (primary, bundle) = bundles.get(name).ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.online.program_missing",
                format!("{name} source closure is missing"),
            )
        })?;
        let analysis = compiler.analyze(bundle);
        semantic_models.push(analysis.semantic.ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.online.compile_failed",
                format!("{primary}: semantic model is missing"),
            )
        })?);
        let result = compiler
            .compile(CompilerRequest {
                source: bundle.clone(),
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").map_err(|error| {
                    CorpusProblem::new("carddemo.online.target_invalid", error.to_string())
                })?,
                options: CompileOptions::new(BTreeMap::new()).map_err(|error| {
                    CorpusProblem::new("carddemo.online.options_invalid", error.to_string())
                })?,
            })
            .map_err(|error| {
                CorpusProblem::new(
                    "carddemo.online.compile_failed",
                    format!("{primary}: {error}"),
                )
            })?;
        let artifact = match result {
            CompilerResult::Published { artifact, .. } => artifact,
            CompilerResult::Analysis { diagnostics, .. }
            | CompilerResult::Failed { diagnostics, .. } => {
                return Err(CorpusProblem::new(
                    "carddemo.online.compile_failed",
                    format!(
                        "{primary}: {}",
                        diagnostics.first().map_or("no diagnostic", |diagnostic| {
                            diagnostic.public_message()
                        })
                    ),
                ));
            }
        };
        programs.push(OnlineProgramDefinition::current(name.clone(), &artifact));
    }
    if programs.len() != 18 || transactions.len() != 17 {
        return Err(CorpusProblem::new(
            "carddemo.online.catalog_drift",
            "base program or source-backed transaction count differs",
        ));
    }
    Ok(OnlineApplicationDefinition {
        programs,
        transactions,
        maps: carddemo_base_maps(corpus_dir, &semantic_models)?,
    })
}
