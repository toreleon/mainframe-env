//! Existing owned batch-program installation and runtime controls.
use super::*;

pub(super) struct BatchProgramExercise {
    pub(super) installed_artifacts: usize,
    pub(super) install_replay: bool,
    pub(super) selected_job_routes: usize,
    pub(super) linkage_routes: usize,
    pub(super) file_routes: usize,
    pub(super) abend_controls: usize,
    pub(super) restart_routes: usize,
    pub(super) operator_proof: operator_observations::BatchProof,
}

pub(super) async fn exercise_batch_program_routes(
    definitions: Vec<BatchProgramDefinition>,
    linkage_fixture: BatchProgramDefinition,
    abend_fixture: BatchProgramDefinition,
    wait_jcl: &[u8],
    operator_inputs: &operator_observations::Inputs,
) -> Result<BatchProgramExercise, CorpusProblem> {
    let artifact_root = env::temp_dir().join(format!(
        "mainframe-env-carddemo-batch-programs-{}",
        std::process::id()
    ));
    fs::create_dir(&artifact_root).map_err(|_| {
        CorpusProblem::new(
            "carddemo.batch_program.artifact_owner",
            "fresh owned artifacts were not claimed",
        )
    })?;
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
    let store = Arc::new(MemoryStore::new(Default::default()));
    let secrets = Arc::new(MemorySecretResolver::default());
    let server = ProductServer::open(
        config.clone(),
        store.clone(),
        secrets.clone(),
        default_program_router(),
    )
    .map_err(terminal_problem)?;
    server
        .bootstrap_user("IBMUSER", b"TESTPASS")
        .map_err(terminal_problem)?;
    let mut operator = operator_observations::BatchExpectation::new(operator_inputs, &definitions)?;
    let first = server
        .install_batch_programs(definitions.clone())
        .map_err(terminal_problem)?;
    let replay = server
        .install_batch_programs(definitions)
        .map_err(terminal_problem)?;
    operator.compare_installation(store.as_ref(), &artifact_root)?;
    server
        .install_batch_programs(vec![linkage_fixture, abend_fixture])
        .map_err(terminal_problem)?;
    let mut record = vec![b' '; 350];
    record[..4].copy_from_slice(b"ABCD");
    let mut dataset_sequence = 1u64;
    utility_seed_dataset(
        &server,
        "IBMUSER.TRNX",
        DatasetOrganization::Sequential,
        RecordFormat::Fixed,
        350,
        None,
        vec![record],
        &mut dataset_sequence,
    )?;
    submit_utility_job(
        &server,
        &server.router(),
        std::str::from_utf8(wait_jcl).map_err(|_| {
            CorpusProblem::new(
                "carddemo.batch_program.jcl_invalid",
                "WAITSTEP is not UTF-8",
            )
        })?,
    )
    .await?;
    submit_utility_job(
        &server,
        &server.router(),
        "//LINKJOB JOB CLASS=A\n//RUN EXEC PGM=LINKMAIN\n//TRNXFILE DD DSN=IBMUSER.TRNX,DISP=SHR\n",
    )
    .await?;
    let principal =
        PrincipalId::new("IBMUSER", InvocationLimits::default()).expect("static principal");
    let (jobs, _) = server
        .batch_service()
        .list(Some(&principal), None, 64)
        .map_err(terminal_problem)?;
    let link_job = jobs
        .iter()
        .find(|job| job.name == "LINKJOB")
        .ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.batch_program.link_missing",
                "LINKJOB was not created",
            )
        })?;
    let (output, _) = server
        .batch_service()
        .spool(
            &base_batch_control_invocation()?,
            &link_job.id,
            "SYSPRINT",
            0,
            64,
        )
        .map_err(terminal_problem)?;
    if !output.iter().any(|record| record == b"00")
        || !output.iter().any(|record| record.starts_with(b"ABCD"))
    {
        return Err(CorpusProblem::new(
            "carddemo.batch_program.link_drift",
            format!("CBSTM03B linkage output differs: {output:?}"),
        ));
    }
    submit_expected_abend(
        &server,
        &server.router(),
        "//ABENDJOB JOB CLASS=A\n//FAIL EXEC PGM=ABENDCHK\n",
        "ABEND U0999",
    )
    .await?;
    if !server.graceful_shutdown().await {
        return Err(CorpusProblem::new(
            "carddemo.batch_program.shutdown_failed",
            "first server did not shut down",
        ));
    }
    let restarted = ProductServer::open(config, store.clone(), secrets, default_program_router())
        .map_err(terminal_problem)?;
    submit_utility_job(
        &restarted,
        &restarted.router(),
        std::str::from_utf8(wait_jcl).map_err(|_| {
            CorpusProblem::new(
                "carddemo.batch_program.jcl_invalid",
                "WAITSTEP is not UTF-8",
            )
        })?,
    )
    .await?;
    operator.compare_installation(store.as_ref(), &artifact_root)?;
    if !restarted.graceful_shutdown().await {
        return Err(CorpusProblem::new(
            "carddemo.batch_program.shutdown_failed",
            "restarted server did not shut down",
        ));
    }
    fs::remove_dir_all(&artifact_root).map_err(|_| {
        CorpusProblem::new(
            "carddemo.batch_program.cleanup_failed",
            "owned artifacts were retained",
        )
    })?;
    let operator_proof = operator.finish()?;
    Ok(BatchProgramExercise {
        installed_artifacts: first.programs,
        install_replay: replay.replayed && replay.identity == first.identity,
        selected_job_routes: 4,
        linkage_routes: 1,
        file_routes: 3,
        abend_controls: 1,
        restart_routes: 1,
        operator_proof,
    })
}
