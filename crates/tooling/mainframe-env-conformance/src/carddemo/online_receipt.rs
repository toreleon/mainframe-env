use super::*;

pub fn verify_carddemo_base_online_from_env(
    inventory_path: &Path,
) -> Result<CardDemoBaseOnlineReceipt, CorpusProblem> {
    verify_carddemo_base_online_observed(inventory_path).map(|(receipt, _)| receipt)
}

pub(super) fn verify_carddemo_base_online_observed(
    inventory_path: &Path,
) -> Result<(CardDemoBaseOnlineReceipt, RouteObservations), CorpusProblem> {
    let terminal = verify_carddemo_terminal_from_env(inventory_path)?;
    let corpus_dir = env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDDEMO_CORPUS_DIR is required",
        )
    })?;
    let definition = carddemo_base_online_definition(Path::new(&corpus_dir))?;
    let programs = definition.programs.len();
    let transactions = definition.transactions.len();
    let maps = definition.maps.len();
    let artifact_root = env::temp_dir().join(format!(
        "mainframe-env-carddemo-base-online-{}",
        std::process::id()
    ));
    let config = ServerConfig {
        store_profile: StoreProfile::Memory,
        artifact_root: artifact_root.clone(),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CorpusProblem::new("carddemo.online.runtime", error.to_string()))?;
    let store = Arc::new(MemoryStore::new(Default::default()));
    let secrets = Arc::new(MemorySecretResolver::default());
    let result = runtime.block_on(exercise_base_online_smoke(
        config,
        store,
        secrets,
        Path::new(&corpus_dir).to_path_buf(),
        definition,
    ));
    let _ = fs::remove_dir_all(&artifact_root);
    let exercise = result?;
    if exercise.screen_paths != maps
        || exercise.dataset_reads == 0
        || exercise.committed_mutations < 6
        || exercise.rollback_controls == 0
        || exercise.denial_controls == 0
        || exercise.restart_controls == 0
        || exercise.concurrency_controls == 0
        || exercise.resource_controls == 0
    {
        return Err(CorpusProblem::new(
            "carddemo.online.coverage_drift",
            format!(
                "screens={}; reads={}; mutations={}; rollback={}; denial={}; restart={}; concurrency={}; resources={}",
                exercise.screen_paths,
                exercise.dataset_reads,
                exercise.committed_mutations,
                exercise.rollback_controls,
                exercise.denial_controls,
                exercise.restart_controls,
                exercise.concurrency_controls,
                exercise.resource_controls
            ),
        ));
    }
    let mut shape = Sha256::new();
    digest_field(&mut shape, terminal.corpus_commit.as_bytes());
    digest_field(&mut shape, &(programs as u64).to_be_bytes());
    digest_field(&mut shape, &(transactions as u64).to_be_bytes());
    digest_field(&mut shape, &(maps as u64).to_be_bytes());
    for value in [
        exercise.initial_screen_bytes,
        exercise.screen_paths,
        exercise.dataset_reads,
        exercise.committed_mutations,
        exercise.rollback_controls,
        exercise.denial_controls,
        exercise.restart_controls,
        exercise.concurrency_controls,
        exercise.resource_controls,
    ] {
        digest_field(&mut shape, &(value as u64).to_be_bytes());
    }
    for observation in &exercise.observations {
        digest_field(&mut shape, observation.as_bytes());
    }
    let route_observations = exercise.route_observations;
    Ok((
        CardDemoBaseOnlineReceipt {
            schema_version: "mainframe-env.carddemo-base-online-receipt@1".into(),
            status: "pass".into(),
            corpus_commit: terminal.corpus_commit,
            journeys_passed: 9,
            programs_installed: programs,
            source_backed_transactions: transactions,
            maps_installed: maps,
            initial_screen_bytes: exercise.initial_screen_bytes,
            screen_paths: exercise.screen_paths,
            dataset_reads: exercise.dataset_reads,
            committed_mutations: exercise.committed_mutations,
            rollback_controls: exercise.rollback_controls,
            denial_controls: exercise.denial_controls,
            restart_controls: exercise.restart_controls,
            concurrency_controls: exercise.concurrency_controls,
            resource_controls: exercise.resource_controls,
            install_replay: exercise.install_replay,
            journey_shape_sha256: format!("{:x}", shape.finalize()),
        },
        route_observations,
    ))
}
