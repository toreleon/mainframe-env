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
        let (program, semantic) = compile_online_program(&compiler, name, primary, bundle)?;
        semantic_models.push(semantic);
        programs.push(program);
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

fn compile_online_program(
    compiler: &CobolCompiler,
    name: &str,
    primary: &str,
    bundle: &SourceBundle,
) -> Result<(OnlineProgramDefinition, SemanticModel), CorpusProblem> {
    let analysis = compiler.analyze(bundle);
    let semantic = analysis.semantic.ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.online.compile_failed",
            format!("{primary}: semantic model is missing"),
        )
    })?;
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
    Ok((
        OnlineProgramDefinition::current(name.to_string(), &artifact),
        semantic,
    ))
}

pub(super) fn transaction_online_definition(
    corpus_dir: &Path,
) -> Result<OnlineApplicationDefinition, CorpusProblem> {
    // Select source closure before invoking the compiler or loading any map.
    let map_paths = [
        "app/bms/COSGN00.bms",
        "app/bms/COMEN01.bms",
        "app/bms/COTRN02.bms",
    ];
    let bundles = source_input::transaction_bundles(corpus_dir)?;
    let csd = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/csd/CARDDEMO.CSD"),
    )?)
    .map_err(|_| CorpusProblem::new("carddemo.online.csd_invalid", "base CSD is not UTF-8"))?;
    let resources = parse_csd(&csd).map_err(package_problem)?;
    let mut transactions = BTreeMap::new();
    for (transaction, expected_program) in [
        ("CC00", "COSGN00C"),
        ("CM00", "COMEN01C"),
        ("CT02", "COTRN02C"),
    ] {
        let matching = resources
            .iter()
            .filter(|resource| resource.kind == "TRANSACTION" && resource.name == transaction)
            .collect::<Vec<_>>();
        if matching.len() != 1
            || matching[0].properties.get("PROGRAM").map(String::as_str) != Some(expected_program)
        {
            return Err(CorpusProblem::new(
                "carddemo.transaction.csd_drift",
                transaction,
            ));
        }
        transactions.insert(transaction.into(), expected_program.into());
    }
    let compiler = CobolCompiler::default();
    let mut programs = Vec::new();
    let mut semantic_models = Vec::new();
    for (primary, bundle) in bundles {
        let name = Path::new(&primary)
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| CorpusProblem::new("carddemo.online.program_invalid", "invalid name"))?;
        let (program, semantic) = compile_online_program(&compiler, name, &primary, &bundle)?;
        programs.push(program);
        semantic_models.push(semantic);
    }
    let maps = map_paths
        .into_iter()
        .map(|path| bms::carddemo_map(corpus_dir, path, &semantic_models))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(OnlineApplicationDefinition {
        programs,
        transactions,
        maps,
    })
}

pub(super) fn navigation_online_definition(
    corpus_dir: &Path,
) -> Result<OnlineApplicationDefinition, CorpusProblem> {
    // Select source closure before invoking the compiler or loading any map.
    let map_paths = [
        "app/bms/COSGN00.bms",
        "app/bms/COMEN01.bms",
        "app/bms/COTRN00.bms",
        "app/bms/COTRN01.bms",
        "app/bms/COTRN02.bms",
    ];
    let bundles = source_input::navigation_bundles(corpus_dir)?;
    let csd = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/csd/CARDDEMO.CSD"),
    )?)
    .map_err(|_| CorpusProblem::new("carddemo.online.csd_invalid", "base CSD is not UTF-8"))?;
    let resources = parse_csd(&csd).map_err(package_problem)?;
    let mut transactions = BTreeMap::new();
    for (transaction, expected_program) in [
        ("CC00", "COSGN00C"),
        ("CM00", "COMEN01C"),
        ("CT00", "COTRN00C"),
        ("CT01", "COTRN01C"),
        ("CT02", "COTRN02C"),
    ] {
        let matching = resources
            .iter()
            .filter(|resource| resource.kind == "TRANSACTION" && resource.name == transaction)
            .collect::<Vec<_>>();
        if matching.len() != 1
            || matching[0].properties.get("PROGRAM").map(String::as_str) != Some(expected_program)
            || matching[0].properties.get("STATUS").map(String::as_str) != Some("ENABLED")
        {
            return Err(CorpusProblem::new(
                "carddemo.transaction.csd_drift",
                transaction,
            ));
        }
        transactions.insert(transaction.into(), expected_program.into());
    }
    for (kind, names) in [
        (
            "PROGRAM",
            &["COSGN00C", "COMEN01C", "COTRN00C", "COTRN01C", "COTRN02C"][..],
        ),
        (
            "MAPSET",
            &["COSGN00", "COMEN01", "COTRN00", "COTRN01", "COTRN02"][..],
        ),
    ] {
        for name in names {
            let matching = resources
                .iter()
                .filter(|r| r.kind == kind && r.name == *name)
                .collect::<Vec<_>>();
            if matching.len() != 1
                || matching[0].properties.get("STATUS").map(String::as_str) != Some("ENABLED")
            {
                return Err(CorpusProblem::new("carddemo.navigation.csd_drift", *name));
            }
        }
    }
    let compiler = CobolCompiler::default();
    let mut programs = Vec::new();
    let mut semantic_models = Vec::new();
    for (primary, bundle) in bundles {
        let name = Path::new(&primary)
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| CorpusProblem::new("carddemo.online.program_invalid", "invalid name"))?;
        let (program, semantic) = compile_online_program(&compiler, name, &primary, &bundle)?;
        programs.push(program);
        semantic_models.push(semantic);
    }
    let maps = map_paths
        .into_iter()
        .map(|path| bms::carddemo_map(corpus_dir, path, &semantic_models))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(OnlineApplicationDefinition {
        programs,
        transactions,
        maps,
    })
}
