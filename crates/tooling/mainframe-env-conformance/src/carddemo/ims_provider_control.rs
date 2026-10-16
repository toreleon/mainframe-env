//! Original provider/controller route, with no first-party observation credit.
use super::*;

pub(super) fn verify_from_env(inventory_path: &Path) -> Result<CardDemoImsReceipt, CorpusProblem> {
    let corpus_dir = PathBuf::from(env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDDEMO_CORPUS_DIR is required",
        )
    })?);
    let corpus = verify_carddemo_corpus(&corpus_dir, inventory_path)?;
    let (definition, definitions_checked) = carddemo_ims_definition(&corpus_dir)?;
    let compiler = CobolCompiler::default();
    let mut programs_compiled = 0usize;
    let mut dli_operations = BTreeMap::new();
    let mut packaged_programs = Vec::new();
    for (relative, bundle) in explicit_carddemo_bundles(&corpus_dir)?
        .into_iter()
        .filter(|(relative, _)| relative.starts_with("app/app-authorization-ims-db2-mq/cbl/"))
    {
        let analysis = compiler.analyze(&bundle);
        if analysis.completeness != Completeness::Complete {
            return Err(CorpusProblem::new(
                "carddemo.ims.compile_failed",
                format!(
                    "{relative}: {}",
                    analysis
                        .diagnostics
                        .first()
                        .map_or("incomplete IMS compilation", |problem| problem
                            .public_message())
                ),
            ));
        }
        let operations = analysis
            .hir
            .ok_or_else(|| CorpusProblem::new("carddemo.ims.compile_failed", "IMS HIR missing"))?
            .statements
            .into_iter()
            .filter(|statement| statement.kind == StatementKind::ExecDli)
            .filter_map(|statement| {
                statement
                    .arguments
                    .into_iter()
                    .find(|token| !matches!(token.as_str(), "DLI" | "END-EXEC"))
            })
            .collect::<Vec<_>>();
        if operations.is_empty() {
            continue;
        }
        for operation in operations {
            *dli_operations.entry(operation).or_default() += 1;
        }
        let result = compiler
            .compile(CompilerRequest {
                source: bundle,
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").expect("static target"),
                options: CompileOptions::new(BTreeMap::new()).expect("static options"),
            })
            .map_err(|problem| {
                CorpusProblem::new(
                    "carddemo.ims.compile_failed",
                    format!("{relative}: {problem:?}"),
                )
            })?;
        let CompilerResult::Published { artifact, .. } = result else {
            return Err(CorpusProblem::new(
                "carddemo.ims.compile_failed",
                format!("{relative} did not publish"),
            ));
        };
        let name = Path::new(&relative)
            .file_stem()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                CorpusProblem::new("carddemo.ims.compile_failed", "program name missing")
            })?;
        packaged_programs.push(BatchProgramDefinition::current(name, &artifact));
        programs_compiled += 1;
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CorpusProblem::new("carddemo.ims.runtime", error.to_string()))?;
    let exercise = runtime.block_on(exercise_ims_routes(
        &corpus_dir,
        StoreProfile::Memory,
        &packaged_programs,
    ))?;
    let sqlite = runtime.block_on(exercise_ims_routes(
        &corpus_dir,
        StoreProfile::Sqlite,
        &packaged_programs,
    ))?;
    if exercise != sqlite {
        return Err(CorpusProblem::new(
            "carddemo.ims.backend_drift",
            "Memory/SQLite observables differ",
        ));
    }
    let mut shape = Sha256::new();
    digest_field(&mut shape, corpus.commit.as_bytes());
    digest_field(
        &mut shape,
        &serde_json::to_vec(&definition)
            .map_err(|error| CorpusProblem::new("carddemo.ims.definition", error.to_string()))?,
    );
    for (operation, count) in &dli_operations {
        digest_field(&mut shape, operation.as_bytes());
        digest_field(&mut shape, &(*count as u64).to_be_bytes());
    }
    digest_field(&mut shape, exercise.hierarchy_sha256.as_bytes());
    for (name, digest) in &exercise.spool_sha256 {
        digest_field(&mut shape, name.as_bytes());
        digest_field(&mut shape, digest.as_bytes());
    }
    Ok(ims_receipt::provider_control_receipt(
        corpus.commit,
        definitions_checked,
        programs_compiled,
        dli_operations,
        exercise,
        format!("{:x}", shape.finalize()),
    )
    .0)
}
