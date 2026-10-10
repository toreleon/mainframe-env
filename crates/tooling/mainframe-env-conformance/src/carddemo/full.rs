use super::*;

pub fn verify_carddemo_full_from_env(
    inventory_path: &Path,
) -> Result<CardDemoFullReceipt, CorpusProblem> {
    let authority = journey_closure::ClosureAuthority::load(inventory_path)?;
    let mut route_observations = RouteObservations::default();
    let corpus_dir = PathBuf::from(env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDDEMO_CORPUS_DIR is required",
        )
    })?);
    let corpus = verify_carddemo_corpus(&corpus_dir, inventory_path)?;
    let cdv1 = verify_cdv1_correction(inventory_path, &corpus.commit)?;
    if env::var_os("MAINFRAME_ENV_POSTGRES_TEST_URL").is_none() {
        return Err(CorpusProblem::new(
            "carddemo.full.postgres_environment_missing",
            "MAINFRAME_ENV_POSTGRES_TEST_URL is required for CardDemo-full certification",
        ));
    }

    let package = verify_carddemo_application_package_from_env(inventory_path)?;
    let resources = verify_carddemo_resources_from_env(inventory_path)?;
    if resources.unresolved != ["CDV1->COCRDSEC"] {
        return Err(CorpusProblem::new(
            "carddemo.full.resource_correction_drift",
            "the accepted COCRDSEC source no longer resolves the sole pinned CSD orphan",
        ));
    }
    let (online, online_observations) =
        online_receipt::verify_carddemo_base_online_observed(inventory_path)?;
    route_observations.extend(online_observations)?;
    let batch = verify_carddemo_base_batch_from_env(inventory_path)?;
    let (db2, db2_observations) = verify_carddemo_db2_observed(inventory_path)?;
    route_observations.extend(db2_observations)?;
    let ims = verify_carddemo_ims_from_env(inventory_path)?;
    let (mq, mq_observations) =
        mq_receipt::verify_carddemo_mq_authorization_observed(inventory_path)?;
    route_observations.extend(mq_observations)?;
    let mut profile_receipt_sha256 = BTreeMap::<String, String>::new();
    for (profile, value) in [
        (
            "application-package",
            serde_json::to_value(&package).map_err(full_json_problem)?,
        ),
        (
            "application-resources",
            serde_json::to_value(&resources).map_err(full_json_problem)?,
        ),
        (
            "carddemo-base-online",
            serde_json::to_value(&online).map_err(full_json_problem)?,
        ),
        (
            "carddemo-base",
            serde_json::to_value(&batch).map_err(full_json_problem)?,
        ),
        (
            "carddemo-db2",
            serde_json::to_value(&db2).map_err(full_json_problem)?,
        ),
        (
            "carddemo-ims",
            serde_json::to_value(&ims).map_err(full_json_problem)?,
        ),
        (
            "carddemo-authorization",
            serde_json::to_value(&mq).map_err(full_json_problem)?,
        ),
    ] {
        profile_receipt_sha256.insert(profile.into(), json_value_digest(&value)?);
    }
    let operator = carddemo_operator_mapping(&corpus_dir)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CorpusProblem::new("carddemo.full.runtime", error.to_string()))?;
    let exercise = runtime.block_on(exercise_full_certification())?;
    route_observations.extend(exercise.route_observations)?;
    let (journeys_passed, issues_input_passed) = authority.finish(&route_observations)?;
    let release_disposition = "source-checkout; carddemo-conformance-only".to_string();
    let owned_commands = vec![
        "cargo xtask carddemo-operator-install --check".into(),
        "cargo xtask carddemo-operator-compile --check".into(),
        "cargo xtask carddemo-operator-submit --check".into(),
        "cargo xtask carddemo-operator-reset --check".into(),
    ];
    let provider_failure_controls =
        db2.failure_controls + ims.provider_failure_controls + mq.authorization_controls;
    let unknown_outcome_controls = mq.unknown_outcome_controls + batch.rollback_controls;
    let cancellation_controls = batch.cancellation_controls;
    let mut shape = Sha256::new();
    digest_field(&mut shape, corpus.commit.as_bytes());
    for (profile, digest) in &profile_receipt_sha256 {
        digest_field(&mut shape, profile.as_bytes());
        digest_field(&mut shape, digest.as_bytes());
    }
    digest_field(&mut shape, operator.sha256.as_bytes());
    digest_field(
        &mut shape,
        &(exercise.mixed_requests_offered as u64).to_be_bytes(),
    );
    digest_field(
        &mut shape,
        &(exercise.mixed_requests_completed as u64).to_be_bytes(),
    );
    digest_field(&mut shape, cdv1.disposition.as_bytes());
    digest_field(&mut shape, cdv1.correction_sha256.as_bytes());
    digest_field(&mut shape, cdv1.source_sha256.as_bytes());
    digest_field(&mut shape, cdv1.artifact_sha256.as_bytes());
    digest_field(&mut shape, cdv1.screen_sha256.as_bytes());
    digest_field(&mut shape, release_disposition.as_bytes());
    Ok(CardDemoFullReceipt {
        schema_version: "mainframe-env.carddemo-full-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: corpus.commit,
        issues_input_passed,
        journeys_passed,
        profile_receipt_sha256,
        operator_scripts_checked: operator.scripts,
        operator_jcl_submissions: operator.jcl_submissions,
        owned_commands,
        ftp_jes_mappings: 2,
        public_operator_routes: 4,
        mixed_requests_offered: exercise.mixed_requests_offered,
        mixed_requests_completed: exercise.mixed_requests_completed,
        sqlite_backup_restore_controls: exercise.sqlite_backup_restore_controls,
        postgres_restart_controls: exercise.postgres_restart_controls,
        provider_failure_controls,
        unknown_outcome_controls,
        cancellation_controls,
        cross_principal_controls: exercise.cross_principal_controls,
        cdv1_disposition: cdv1.disposition,
        cdv1_correction_sha256: cdv1.correction_sha256,
        cdv1_source_sha256: cdv1.source_sha256,
        cdv1_artifact_sha256: cdv1.artifact_sha256,
        cdv1_screen_sha256: cdv1.screen_sha256,
        cdv1_public_routes: cdv1.public_routes,
        release_disposition,
        native_or_legacy_fallback_present: false,
        operator_mapping_sha256: operator.sha256,
        full_shape_sha256: format!("{:x}", shape.finalize()),
    })
}

struct OperatorMapping {
    scripts: usize,
    jcl_submissions: usize,
    sha256: String,
}

fn carddemo_operator_mapping(corpus_dir: &Path) -> Result<OperatorMapping, CorpusProblem> {
    let scripts = [
        "scripts/local_compile.sh",
        "scripts/remote_compile.sh",
        "scripts/remote_refresh.sh",
        "scripts/remote_submit.sh",
        "scripts/run_full_batch.sh",
        "scripts/run_interest_calc.sh",
        "scripts/run_posting.sh",
        "scripts/upld_module.sh",
    ];
    let mut digest = Sha256::new();
    let mut submissions = 0usize;
    for relative in scripts {
        let bytes = read_corpus_file(corpus_dir, &corpus_dir.join(relative))?;
        let text = std::str::from_utf8(&bytes).map_err(|_| {
            CorpusProblem::new(
                "carddemo.full.operator_script_invalid",
                format!("{relative} is not UTF-8"),
            )
        })?;
        submissions += text
            .lines()
            .map(str::trim)
            .filter(|line| {
                line.to_ascii_lowercase().starts_with("put ")
                    && line.to_ascii_lowercase().contains(".jcl")
            })
            .count();
        digest_field(&mut digest, relative.as_bytes());
        digest_field(&mut digest, &bytes);
    }
    let ftp_relative = "app/jcl/FTPJCL.JCL";
    let ftp = read_corpus_file(corpus_dir, &corpus_dir.join(ftp_relative))?;
    let ftp_text = String::from_utf8_lossy(&ftp).to_ascii_uppercase();
    if submissions == 0
        || !ftp_text.contains("PGM=FTP")
        || !ftp_text.contains("PUT 'AWS.M2.CARDEMO.FTP.TEST' WELCOME.TXT")
    {
        return Err(CorpusProblem::new(
            "carddemo.full.operator_mapping_drift",
            "pinned FTP/JES operator workflow changed",
        ));
    }
    digest_field(&mut digest, ftp_relative.as_bytes());
    digest_field(&mut digest, &ftp);
    digest_field(
        &mut digest,
        b"FTP SITE FILETYPE=JES PUT -> PUT /zosmf/restjobs/jobs",
    );
    digest_field(
        &mut digest,
        b"FTP PUT AWS.M2.CARDEMO.FTP.TEST -> GET /zosmf/restfiles/ds/AWS.M2.CARDDEMO.FTP.TEST",
    );
    Ok(OperatorMapping {
        scripts: scripts.len() + 1,
        jcl_submissions: submissions,
        sha256: format!("{:x}", digest.finalize()),
    })
}

pub(super) struct FullCertificationExercise {
    pub(super) mixed_requests_offered: usize,
    pub(super) mixed_requests_completed: usize,
    pub(super) sqlite_backup_restore_controls: usize,
    pub(super) postgres_restart_controls: usize,
    pub(super) cross_principal_controls: usize,
    pub(super) route_observations: RouteObservations,
}

pub(super) async fn exercise_full_certification() -> Result<FullCertificationExercise, CorpusProblem>
{
    let mut route_observations = RouteObservations::default();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| CorpusProblem::new("carddemo.full.clock", error.to_string()))?
        .as_nanos();
    let memory_root = env::temp_dir().join(format!("mainframe-env-carddemo-full-memory-{nonce}"));
    fs::create_dir(&memory_root)
        .map_err(|error| CorpusProblem::new("carddemo.full.memory_owner", error.to_string()))?;
    let memory = ProductServer::open(
        ServerConfig {
            store_profile: StoreProfile::Memory,
            artifact_root: memory_root.clone(),
            max_concurrency: 8,
            tls: TlsConfig {
                enabled: false,
                certificate_path: None,
                private_key_reference: None,
            },
            ..ServerConfig::default()
        },
        Arc::new(MemoryStore::new(Default::default())),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
    )
    .map_err(terminal_problem)?;
    memory
        .bootstrap_user("IBMUSER", b"TESTPASS")
        .map_err(terminal_problem)?;
    memory
        .bootstrap_identity("APPUSER", b"APPPASS1")
        .map_err(terminal_problem)?;
    memory
        .racf_service()
        .permit("JESJOBS", "JOB.**", "APPUSER", AccessIntent::Alter)
        .map_err(terminal_problem)?;
    memory
        .start_background_workers()
        .map_err(terminal_problem)?;
    let app = memory.router();
    let basic = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("IBMUSER:TESTPASS")
    );
    memory
        .racf_service()
        .define_profile("DATASET", "AWS.M2.CARDDEMO.FTP.TEST", "IBMUSER", None)
        .map_err(terminal_problem)?;
    memory
        .racf_service()
        .permit(
            "DATASET",
            "AWS.M2.CARDDEMO.FTP.TEST",
            "IBMUSER",
            AccessIntent::Read,
        )
        .map_err(terminal_problem)?;
    let mut ftp_sequence = u64::try_from(nonce % 1_000_000_000)
        .map_err(|_| CorpusProblem::new("carddemo.full.ftp", "sequence overflow"))?;
    utility_seed_dataset(
        &memory,
        "AWS.M2.CARDDEMO.FTP.TEST",
        DatasetOrganization::Sequential,
        RecordFormat::Fixed,
        16,
        None,
        vec![b"WELCOME-CD027   ".to_vec()],
        &mut ftp_sequence,
    )?;
    let (ftp_status, ftp_bytes) = terminal_http(
        &app,
        Method::GET,
        "/zosmf/restfiles/ds/AWS.M2.CARDDEMO.FTP.TEST",
        BTreeMap::from([("authorization".into(), basic.clone())]),
        Vec::new(),
    )
    .await?;
    if ftp_status != StatusCode::OK || ftp_bytes != b"WELCOME-CD027" {
        return Err(CorpusProblem::new(
            "carddemo.full.ftp_mapping_drift",
            "owned dataset download did not preserve FTPJCL bytes",
        ));
    }
    let mut tasks = Vec::new();
    for index in 0..8usize {
        let route = app.clone();
        tasks.push(tokio::spawn(async move {
            (
                None::<String>,
                terminal_http(
                    &route,
                    Method::GET,
                    "/zosmf/info",
                    BTreeMap::new(),
                    Vec::new(),
                )
                .await,
            )
        }));
        let route = app.clone();
        let authorization = basic.clone();
        tasks.push(tokio::spawn(async move {
            let name = format!("MX{index:06}");
            let jcl = format!("//{name} JOB 'CD027',CLASS=A,MSGCLASS=H\n//STEP EXEC PGM=IEFBR14\n");
            (
                Some(name),
                terminal_http(
                    &route,
                    Method::PUT,
                    "/zosmf/restjobs/jobs",
                    BTreeMap::from([
                        ("authorization".into(), authorization),
                        ("x-csrf-zosmf-header".into(), "true".into()),
                    ]),
                    jcl.into_bytes(),
                )
                .await,
            )
        }));
    }
    // Join every offered request before interpreting any failure.
    let mut joined = Vec::new();
    for task in tasks {
        joined.push(task.await);
    }
    let mut completed = 0usize;
    let mut info_ok = 0usize;
    let mut submitted_jobs = BTreeMap::new();
    let mut submitted_ids = BTreeSet::new();
    for result in joined {
        let (expected_name, response) = result.map_err(|error| {
            CorpusProblem::new("carddemo.full.overload_drift", error.to_string())
        })?;
        let (status, body) = response?;
        // Preserve the original status union, then enforce each actual route.
        if !matches!(status, StatusCode::OK | StatusCode::CREATED) {
            return Err(CorpusProblem::new(
                "carddemo.full.overload_drift",
                format!("mixed-load request returned {status}"),
            ));
        }
        completed += 1;
        match expected_name {
            None => {
                require_terminal_status(status, StatusCode::OK, "mixed-load info")?;
                info_ok += 1;
            }
            Some(name) => {
                require_terminal_status(status, StatusCode::CREATED, "mixed-load job submission")?;
                let job: serde_json::Value = serde_json::from_slice(&body).map_err(|error| {
                    CorpusProblem::new("carddemo.full.overload_drift", error.to_string())
                })?;
                let id = job["jobid"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| {
                        CorpusProblem::new(
                            "carddemo.full.overload_drift",
                            "submitted job ID is missing",
                        )
                    })?
                    .to_string();
                if job["jobname"] != name
                    || job["owner"] != "IBMUSER"
                    || !submitted_ids.insert(id)
                    || submitted_jobs.insert(name, job).is_some()
                {
                    return Err(CorpusProblem::new(
                        "carddemo.full.overload_drift",
                        "mixed-load submissions did not produce distinct expected owner job identities",
                    ));
                }
            }
        }
    }
    if completed != 16 || memory.metrics().active != 0 {
        return Err(CorpusProblem::new(
            "carddemo.full.overload_drift",
            format!("2x mixed load completed {completed}/16"),
        ));
    }
    let owner_headers = BTreeMap::from([("authorization".into(), basic.clone())]);
    for (name, job) in &submitted_jobs {
        let actual = wait_for_submitted_job(&memory, &app, &owner_headers, job.clone()).await?;
        if actual["jobname"] != *name
            || actual["jobid"] != job["jobid"]
            || actual["owner"] != "IBMUSER"
            || actual["status"] != "OUTPUT"
            || actual["retcode"] != "CC 0000"
        {
            return Err(CorpusProblem::new(
                "carddemo.full.overload_drift",
                "mixed-load owner jobs did not complete with their exact submitted identities",
            ));
        }
    }
    let mixed_active = memory.metrics().active;
    let mixed_refused = completed != 16
        || info_ok != 8
        || submitted_jobs.len() != 8
        || submitted_ids.len() != 8
        || mixed_active != 0;
    let appuser = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("APPUSER:APPPASS1")
    );
    let job_spool_proof = jes_security_observations::exercise_job_principal_denials(
        &app,
        &owner_headers,
        &appuser,
        &submitted_jobs,
    )
    .await?;
    let principal_refused = job_spool_proof.principal_refused();
    let (status, body) = terminal_http(
        &app,
        Method::GET,
        "/zosmf/restjobs/jobs",
        BTreeMap::from([("authorization".into(), appuser)]),
        Vec::new(),
    )
    .await?;
    if status != StatusCode::OK
        || body.windows(8).any(|window| window == b"MX000000")
        || principal_refused
    {
        return Err(CorpusProblem::new(
            "carddemo.full.principal_leak",
            "APPUSER observed IBMUSER job state",
        ));
    }
    let (invalid_status, _) = terminal_http(
        &app,
        Method::GET,
        "/zosmf/restjobs/jobs",
        BTreeMap::from([(
            "authorization".into(),
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode("IBMUSER:WRONG")
            ),
        )]),
        Vec::new(),
    )
    .await?;
    let memory_drained = memory.graceful_shutdown().await;
    route_observations.compare(
        journey_closure::AuthorityKind::Journey,
        "CD.J19",
        "authentication",
        invalid_status != StatusCode::UNAUTHORIZED || !memory_drained,
        || {
            Ok(CorpusProblem::new(
                "carddemo.full.authentication_drift",
                "invalid credentials or memory shutdown did not fail closed",
            ))
        },
    )?;
    drop(app);
    drop(memory);
    fs::remove_dir_all(&memory_root)
        .map_err(|error| CorpusProblem::new("carddemo.full.memory_cleanup", error.to_string()))?;
    // Publish positive owner evidence after actual drain and owned cleanup.
    job_spool_proof.observe_job_ids_and_spool(&mut route_observations)?;
    route_observations.compare(
        journey_closure::AuthorityKind::Journey,
        "CD.J20",
        "2x overload",
        mixed_refused,
        || {
            Ok(CorpusProblem::new(
                "carddemo.full.overload_drift",
                "16 joined mixed requests did not retain eight info results and eight completed owner job identities",
            ))
        },
    )?;

    let sqlite_root = env::temp_dir().join(format!("mainframe-env-carddemo-full-sqlite-{nonce}"));
    fs::create_dir(&sqlite_root)
        .map_err(|error| CorpusProblem::new("carddemo.full.sqlite", error.to_string()))?;
    let sqlite_path = sqlite_root.join("state.db");
    let sqlite_backup = sqlite_root.join("backup.db");
    let sqlite_url = format!("sqlite://{}?mode=rwc", sqlite_path.display());
    let sqlite_store = Arc::new(
        SqliteStateStore::open(&sqlite_url, 64 * 1024 * 1024, 262_144)
            .map_err(|error| CorpusProblem::new("carddemo.full.sqlite", error.to_string()))?,
    );
    let sqlite_config = ServerConfig {
        store_profile: StoreProfile::Sqlite,
        sqlite_url: sqlite_url.clone(),
        artifact_root: sqlite_root.join("artifacts"),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    let sqlite_server = ProductServer::open(
        sqlite_config,
        sqlite_store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
    )
    .map_err(terminal_problem)?;
    let mut sqlite_sequence = u64::try_from(nonce % 1_000_000_000)
        .map_err(|_| CorpusProblem::new("carddemo.full.sqlite", "sequence overflow"))?;
    utility_seed_dataset(
        &sqlite_server,
        "IBMUSER.CD027.BACKUP",
        DatasetOrganization::Sequential,
        RecordFormat::Fixed,
        16,
        None,
        vec![b"SQLITE-RESTORE  ".to_vec()],
        &mut sqlite_sequence,
    )?;
    if !sqlite_server.graceful_shutdown().await {
        return Err(CorpusProblem::new(
            "carddemo.full.sqlite",
            "SQLite server did not drain",
        ));
    }
    drop(sqlite_server);
    sqlite_store
        .integrity_check()
        .map_err(|error| CorpusProblem::new("carddemo.full.sqlite", error.to_string()))?;
    sqlite_store
        .backup_to(&sqlite_backup)
        .map_err(|error| CorpusProblem::new("carddemo.full.sqlite", error.to_string()))?;
    drop(sqlite_store);
    let restored_url = format!("sqlite://{}?mode=rw", sqlite_backup.display());
    let restored_store = Arc::new(
        SqliteStateStore::open(&restored_url, 64 * 1024 * 1024, 262_144)
            .map_err(|error| CorpusProblem::new("carddemo.full.sqlite", error.to_string()))?,
    );
    restored_store
        .integrity_check()
        .map_err(|error| CorpusProblem::new("carddemo.full.sqlite", error.to_string()))?;
    let restored = ProductServer::open(
        ServerConfig {
            store_profile: StoreProfile::Sqlite,
            sqlite_url: restored_url,
            artifact_root: sqlite_root.join("restored-artifacts"),
            tls: TlsConfig {
                enabled: false,
                certificate_path: None,
                private_key_reference: None,
            },
            ..ServerConfig::default()
        },
        restored_store,
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
    )
    .map_err(terminal_problem)?;
    // Attempt the actual stop independently of a failed read or byte comparison.
    let restored_records = utility_records(&restored, "IBMUSER.CD027.BACKUP", None);
    let restored_drained = restored.graceful_shutdown().await;
    route_observations.compare(
        journey_closure::AuthorityKind::Journey,
        "CD.J20",
        "backup/restore",
        restored_records? != [b"SQLITE-RESTORE  ".to_vec()] || !restored_drained,
        || {
            Ok(CorpusProblem::new(
                "carddemo.full.sqlite_restore_drift",
                "SQLite backup did not restore exact provider bytes",
            ))
        },
    )?;
    drop(restored);
    fs::remove_dir_all(&sqlite_root)
        .map_err(|error| CorpusProblem::new("carddemo.full.sqlite_cleanup", error.to_string()))?;

    let postgres_url = env::var("MAINFRAME_ENV_POSTGRES_TEST_URL").map_err(|_| {
        CorpusProblem::new(
            "carddemo.full.postgres_environment_missing",
            "PostgreSQL test URL is required",
        )
    })?;
    let postgres_store = Arc::new(
        PostgresStateStore::open(&postgres_url, 64 * 1024 * 1024, 262_144)
            .map_err(|error| CorpusProblem::new("carddemo.full.postgres", error.to_string()))?,
    );
    let postgres_artifacts = Arc::new(
        PostgresArtifactStore::open(&postgres_url, 64 * 1024 * 1024, 262_144)
            .map_err(|error| CorpusProblem::new("carddemo.full.postgres", error.to_string()))?,
    );
    let postgres_root =
        env::temp_dir().join(format!("mainframe-env-carddemo-full-postgres-{nonce}"));
    let postgres_config = ServerConfig {
        store_profile: StoreProfile::Postgres,
        postgres_url_reference: Some("env-base64:MAINFRAME_ENV_SECRET_PG".into()),
        artifact_profile: ArtifactProfile::Shared,
        artifact_root: postgres_root.clone(),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    let postgres = ProductServer::open_with_artifact_store(
        postgres_config.clone(),
        postgres_store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        postgres_artifacts.clone(),
    )
    .map_err(terminal_problem)?;
    if postgres_root.exists() {
        return Err(CorpusProblem::new(
            "carddemo.full.postgres_local_artifact_fallback",
            "PostgreSQL profile created a node-local artifact directory",
        ));
    }
    let dataset = format!("IBMUSER.CD{:06}", nonce % 1_000_000);
    let mut postgres_sequence = u64::try_from((nonce / 1_000_000) % 1_000_000_000)
        .map_err(|_| CorpusProblem::new("carddemo.full.postgres", "sequence overflow"))?;
    utility_seed_dataset(
        &postgres,
        &dataset,
        DatasetOrganization::Sequential,
        RecordFormat::Fixed,
        16,
        None,
        vec![b"POSTGRES-RESTART".to_vec()],
        &mut postgres_sequence,
    )?;
    if !postgres.graceful_shutdown().await {
        return Err(CorpusProblem::new(
            "carddemo.full.postgres",
            "PostgreSQL server did not drain",
        ));
    }
    drop(postgres);
    let postgres_restarted = ProductServer::open_with_artifact_store(
        postgres_config,
        postgres_store,
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        postgres_artifacts,
    )
    .map_err(terminal_problem)?;
    if utility_records(&postgres_restarted, &dataset, None)? != [b"POSTGRES-RESTART".to_vec()] {
        return Err(CorpusProblem::new(
            "carddemo.full.postgres_restart_drift",
            "PostgreSQL restart changed provider bytes",
        ));
    }
    postgres_sequence = postgres_sequence.saturating_add(1);
    postgres_restarted
        .dataset_service()
        .invoke(DatasetRequest::Delete {
            dataset: DatasetName::new(&dataset, 128).expect("bounded test dataset"),
            member: None,
            expected_version: None,
            purge: true,
            current_date: None,
            mutation: Mutation {
                sequence: postgres_sequence,
                idempotency_key: IdempotencyKey::new(
                    format!("carddemo-full-postgres-delete-{nonce}"),
                    InvocationLimits::default(),
                )
                .expect("bounded delete key"),
                transaction: Some("CD-027".into()),
            },
        })
        .map_err(terminal_problem)?;
    if !postgres_restarted.graceful_shutdown().await {
        return Err(CorpusProblem::new(
            "carddemo.full.postgres",
            "reopened PostgreSQL server did not drain",
        ));
    }
    drop(postgres_restarted);
    // Shared PostgreSQL artifacts must never create a local directory. The
    // existing refusal above remains authoritative; this path owns no local
    // PostgreSQL artifact namespace to remove.
    Ok(FullCertificationExercise {
        route_observations,
        mixed_requests_offered: 16,
        mixed_requests_completed: completed,
        sqlite_backup_restore_controls: 1,
        postgres_restart_controls: 1,
        cross_principal_controls: 2,
    })
}

fn full_json_problem(error: serde_json::Error) -> CorpusProblem {
    CorpusProblem::new("carddemo.full.receipt", error.to_string())
}

fn json_value_digest(value: &serde_json::Value) -> Result<String, CorpusProblem> {
    serde_json::to_vec(value)
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
        .map_err(full_json_problem)
}
