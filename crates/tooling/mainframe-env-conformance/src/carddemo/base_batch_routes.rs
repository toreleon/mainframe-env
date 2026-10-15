//! Own the selected base batch exercise and its retained dataset/spool comparisons.

use super::*;

pub(super) async fn exercise_base_batch_routes(
    corpus_dir: &Path,
    corpus_commit: String,
    online: OnlineApplicationDefinition,
    definitions: Vec<BatchProgramDefinition>,
) -> Result<
    (
        CardDemoBaseBatchReceipt,
        RouteObservations,
        base_batch_evidence::FinancialProof,
    ),
    CorpusProblem,
> {
    const BUSINESS_DATE: &str = "2022-07-06";
    const COBOL_CURRENT_DATE: &str = "2022070600000000+0000";
    let mut financial_checks = financial_observations::FinancialChecks::prepare(
        corpus_dir,
        &corpus_commit,
        COBOL_CURRENT_DATE,
    )?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| CorpusProblem::new("carddemo.base_batch.clock", error.to_string()))?
        .as_nanos();
    let artifact_parent = env::temp_dir().canonicalize().map_err(|error| {
        CorpusProblem::new("carddemo.base_batch.artifact_root", error.to_string())
    })?;
    let artifact_root = artifact_parent.join(format!(
        "mainframe-env-carddemo-base-batch-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&artifact_root).map_err(|error| {
        CorpusProblem::new("carddemo.base_batch.artifact_root", error.to_string())
    })?;
    let mut file_controls = batch_observations::FileControls::default();
    let config = ServerConfig {
        store_profile: StoreProfile::Memory,
        cobol_current_date: Some(COBOL_CURRENT_DATE.into()),
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
    install_base_online_authorities(&server, corpus_dir, &online)?;
    server
        .install_online_application(online)
        .map_err(terminal_problem)?;
    server
        .install_batch_programs(definitions)
        .map_err(terminal_problem)?;
    let control_invocation = base_batch_control_invocation()?;
    let racf = server.racf_service();
    for object in carddemo_base_seed_objects(corpus_dir)? {
        racf.permit(
            "DATASET",
            object.dataset.as_str(),
            "IBMUSER",
            AccessIntent::Alter,
        )
        .map_err(terminal_problem)?;
    }
    let csd = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/csd/CARDDEMO.CSD"),
    )?)
    .map_err(|_| CorpusProblem::new("carddemo.base_batch.csd_invalid", "CSD is not UTF-8"))?;
    for dataset in parse_csd(&csd)
        .map_err(package_problem)?
        .into_iter()
        .filter(|resource| resource.kind == "FILE")
        .filter_map(|resource| resource.properties.get("DSNAME").cloned())
    {
        racf.permit("DATASET", &dataset, "IBMUSER", AccessIntent::Alter)
            .map_err(terminal_problem)?;
    }
    racf.define_profile("DATASET", "AWS.M2.CARDDEMO.**", "IBMUSER", None)
        .map_err(terminal_problem)?;
    racf.permit(
        "DATASET",
        "AWS.M2.CARDDEMO.**",
        "IBMUSER",
        AccessIntent::Alter,
    )
    .map_err(terminal_problem)?;
    racf.define_profile("DATASET", "AWS.M2.CARDEMO.**", "IBMUSER", None)
        .map_err(terminal_problem)?;
    racf.permit(
        "DATASET",
        "AWS.M2.CARDEMO.**",
        "IBMUSER",
        AccessIntent::Alter,
    )
    .map_err(terminal_problem)?;
    let app = server.router();
    install_base_batch_source_datasets(&server, corpus_dir)?;
    let mut job_ids = BTreeMap::new();
    for relative in [
        "app/jcl/ACCTFILE.jcl",
        "app/jcl/CARDFILE.jcl",
        "app/jcl/CUSTFILE.jcl",
        "app/jcl/XREFFILE.jcl",
        "app/jcl/TCATBALF.jcl",
        "app/jcl/TRANCATG.jcl",
        "app/jcl/TRANTYPE.jcl",
        "app/jcl/DISCGRP.jcl",
        "app/jcl/DUSRSECJ.jcl",
        "app/jcl/TRANFILE.jcl",
    ] {
        let jcl = String::from_utf8(read_corpus_file(corpus_dir, &corpus_dir.join(relative))?)
            .map_err(|_| {
                CorpusProblem::new(
                    "carddemo.base_batch.jcl_invalid",
                    format!("{relative} is not UTF-8"),
                )
            })?;
        let id = submit_job_with_retcode(&server, &app, &jcl, "CC 0000")
            .await
            .map_err(|problem| {
                CorpusProblem::new(
                    "carddemo.base_batch.job_failed",
                    format!("{relative}: {problem}"),
                )
            })?;
        job_ids.insert(base_batch_job_label(relative)?, id);
    }
    for relative in ["app/jcl/DEFGDGB.jcl", "app/jcl/DALYREJS.jcl"] {
        let jcl = String::from_utf8(read_corpus_file(corpus_dir, &corpus_dir.join(relative))?)
            .map_err(|_| {
                CorpusProblem::new(
                    "carddemo.base_batch.jcl_invalid",
                    format!("{relative} is not UTF-8"),
                )
            })?;
        let id = submit_job_with_retcode(&server, &app, &jcl, "CC 0000")
            .await
            .map_err(|problem| {
                CorpusProblem::new(
                    "carddemo.base_batch.job_failed",
                    format!("{relative}: {problem}"),
                )
            })?;
        job_ids.insert(base_batch_job_label(relative)?, id);
    }
    financial_checks.capture_initial(&server)?;
    let close_jcl = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/jcl/CLOSEFIL.jcl"),
    )?)
    .map_err(|_| {
        CorpusProblem::new(
            "carddemo.base_batch.jcl_invalid",
            "app/jcl/CLOSEFIL.jcl is not UTF-8",
        )
    })?;
    let close_id = submit_job_with_retcode(&server, &app, &close_jcl, "CC 0000").await?;
    job_ids.insert("CLOSEFIL".into(), close_id.clone());
    let mut closed_states = Vec::new();
    for file in ["TRANSACT", "CCXREF", "ACCTDAT", "CXACAIX", "USRSEC"] {
        let state = server
            .cics_service()
            .file_status(file)
            .map_err(terminal_problem)?;
        closed_states.push((file, state));
        if state != CicsFileStatus::Closed {
            return Err(CorpusProblem::new(
                "carddemo.base_batch.cics_close_drift",
                format!("{file} was not closed by CLOSEFIL"),
            ));
        }
    }
    let (closed_output, closed_more) = server
        .batch_service()
        .spool(&control_invocation, &close_id, "CMDOUT", 0, 16)
        .map_err(terminal_problem)?;
    if closed_more
        || closed_output
            != [
                "TRANSACT CLOSED",
                "CCXREF CLOSED",
                "ACCTDAT CLOSED",
                "CXACAIX CLOSED",
                "USRSEC CLOSED",
            ]
            .map(|record| record.as_bytes().to_vec())
    {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.cics_close_drift",
            "CLOSEFIL command output differs",
        ));
    }
    file_controls.compare_closed(
        close_jcl.as_bytes(),
        &closed_states,
        &closed_output,
        closed_more,
    )?;
    for (relative, expected) in [
        ("app/jcl/POSTTRAN.jcl", "CC 0004"),
        ("app/jcl/INTCALC.jcl", "CC 0000"),
        ("app/jcl/TRANBKP.jcl", "CC 0000"),
        ("app/jcl/COMBTRAN.jcl", "CC 0000"),
        ("app/jcl/TRANIDX.jcl", "CC 0000"),
        ("app/jcl/TRANREPT.jcl", "CC 0000"),
        ("app/jcl/PRTCATBL.jcl", "CC 0000"),
    ] {
        let jcl = String::from_utf8(read_corpus_file(corpus_dir, &corpus_dir.join(relative))?)
            .map_err(|_| {
                CorpusProblem::new(
                    "carddemo.base_batch.jcl_invalid",
                    format!("{relative} is not UTF-8"),
                )
            })?;
        financial_observations::require_source_input(relative, jcl.as_bytes())?;
        let id = submit_job_with_retcode(&server, &app, &jcl, expected)
            .await
            .map_err(|problem| {
                CorpusProblem::new(
                    "carddemo.base_batch.job_failed",
                    format!("{relative}: {problem}"),
                )
            })?;
        let label = base_batch_job_label(relative)?;
        financial_checks.job_done(&server, &label, &id)?;
        job_ids.insert(label, id);
        if relative == "app/jcl/TRANBKP.jcl" {
            utility_records(&server, "AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS", None).map_err(
                |problem| {
                    CorpusProblem::new(
                        "carddemo.base_batch.backup_drift",
                        format!("TRANBKP did not recreate the transaction master: {problem}"),
                    )
                },
            )?;
        }
    }
    if let Ok(directory) = env::var("CARDDEMO_TRANREPT_EXTRACT_DIR") {
        let directory = Path::new(&directory);
        if !directory.is_absolute() {
            return Err(CorpusProblem::new(
                "carddemo.base_batch.extract_path",
                "CARDDEMO_TRANREPT_EXTRACT_DIR must be absolute",
            ));
        }
        fs::create_dir_all(directory).map_err(|error| {
            CorpusProblem::new("carddemo.base_batch.extract", error.to_string())
        })?;
        for (dataset, filename) in [
            (
                "AWS.M2.CARDDEMO.TRANSACT.BKUP.G0002V00",
                "tranrept-input.bin",
            ),
            (
                "AWS.M2.CARDDEMO.TRANSACT.DALY.G0001V00",
                "tranrept-selected.bin",
            ),
            ("AWS.M2.CARDDEMO.TRANREPT.G0001V00", "tranrept-report.bin"),
        ] {
            let records = utility_records(&server, dataset, None)?;
            let bytes = records.concat();
            fs::write(directory.join(filename), bytes).map_err(|error| {
                CorpusProblem::new("carddemo.base_batch.extract", error.to_string())
            })?;
            let name = DatasetName::new(dataset, 128).map_err(|_| {
                CorpusProblem::new("carddemo.base_batch.extract", "dataset name is invalid")
            })?;
            let DatasetResult::Attributes {
                attributes,
                version,
            } = server
                .dataset_service()
                .invoke(DatasetRequest::Attributes {
                    dataset: name.clone(),
                })
                .map_err(terminal_problem)?
            else {
                return Err(CorpusProblem::new(
                    "carddemo.base_batch.extract",
                    "attributes unavailable",
                ));
            };
            let DatasetResult::Records {
                version: read_version,
                identities,
                ..
            } = server
                .dataset_service()
                .invoke(DatasetRequest::Read {
                    dataset: name,
                    member: None,
                    key: None,
                    max_records: 4_096,
                    control: Default::default(),
                })
                .map_err(terminal_problem)?
            else {
                return Err(CorpusProblem::new(
                    "carddemo.base_batch.extract",
                    "records unavailable",
                ));
            };
            fs::write(
                directory.join(format!("{filename}.meta")),
                format!(
                    "attributes={:?}\nversion={version}\nread_version={read_version}\nidentities={} first={:?}\n",
                    attributes,
                    identities.len(),
                    identities.first()
                ),
            )
            .map_err(|error| CorpusProblem::new("carddemo.base_batch.extract", error.to_string()))?;
        }
    }
    let tranrept_selected_records =
        utility_records(&server, "AWS.M2.CARDDEMO.TRANSACT.DALY.G0001V00", None)?.len();
    let tranrept_report_records =
        utility_records(&server, "AWS.M2.CARDDEMO.TRANREPT.G0001V00", None)?.len();
    if tranrept_selected_records == 0 || tranrept_report_records == 0 {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.tranrept_empty",
            format!(
                "TRANREPT selected {tranrept_selected_records} records and wrote {tranrept_report_records} report records"
            ),
        ));
    }
    let post_id = job_ids
        .get("POSTTRAN")
        .ok_or_else(|| CorpusProblem::new("carddemo.base_batch.job_missing", "POSTTRAN missing"))?;
    let post_text = base_batch_spool_text(&server, post_id)?;
    let compact_post = post_text
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    if !compact_post.contains("TRANSACTIONSPROCESSED:000000300")
        || !compact_post.contains("TRANSACTIONSREJECTED:000000038")
    {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.posting_drift",
            "POSTTRAN did not report 300 processed and 38 rejected transactions",
        ));
    }
    let statement = corrected_creastmt(corpus_dir)?;
    financial_observations::require_statement_input(statement.as_bytes())?;
    let statement_id = submit_job_with_retcode(&server, &app, &statement, "CC 0000")
        .await
        .map_err(|problem| {
            CorpusProblem::new(
                "carddemo.base_batch.job_failed",
                format!("app/jcl/CREASTMT.JCL: {problem}"),
            )
        })?;
    financial_checks.job_done(&server, "CREASTMT", &statement_id)?;
    job_ids.insert("CREASTMT".into(), statement_id);
    for name in [
        "AWS.M2.CARDDEMO.TRXFL.SEQ",
        "AWS.M2.CARDDEMO.TRXFL.VSAM.KSDS",
        "AWS.M2.CARDDEMO.STATEMNT.PS",
        "AWS.M2.CARDDEMO.STATEMNT.HTML",
        "AWS.M2.CARDDEMO.TCATBALF.REPT",
    ] {
        if utility_records(&server, name, None)?.is_empty() {
            return Err(CorpusProblem::new(
                "carddemo.base_batch.output_empty",
                format!("{name} is empty"),
            ));
        }
    }

    let internal_jcl = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/jcl/INTRDRJ1.JCL"),
    )?)
    .map_err(|_| {
        CorpusProblem::new(
            "carddemo.base_batch.jcl_invalid",
            "app/jcl/INTRDRJ1.JCL is not UTF-8",
        )
    })?;
    let internal_id = submit_job_with_retcode(&server, &app, &internal_jcl, "CC 0000").await?;
    job_ids.insert("INTRDRJ1".into(), internal_id);
    let principal =
        PrincipalId::new("IBMUSER", InvocationLimits::default()).expect("static batch principal");
    let (_, more) = server
        .batch_service()
        .list(Some(&principal), None, 128)
        .map_err(terminal_problem)?;
    if more {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.job_limit",
            "job list exceeded the base-cycle observation bound",
        ));
    }
    let child = wait_for_listed_job(
        &server,
        &principal,
        "INTRDRJ2",
        128,
        "carddemo.base_batch.internal_missing",
        "carddemo.base_batch.internal_incomplete",
    )
    .await?;
    if child.state != JobState::Completed || child.return_code != Some(0) {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.internal_incomplete",
            format!("INTRDRJ2 did not complete: {child:?}"),
        ));
    }
    job_ids.insert("INTRDRJ2".into(), child.id);
    if utility_records(&server, "AWS.M2.CARDEMO.FTP.TEST.BKUP.INTRDR", None)?
        != utility_records(&server, "AWS.M2.CARDEMO.FTP.TEST.BKUP", None)?
    {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.internal_copy_drift",
            "INTRDRJ2 did not reproduce the FTP backup exactly",
        ));
    }

    let open_jcl = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/jcl/OPENFIL.jcl"),
    )?)
    .map_err(|_| {
        CorpusProblem::new(
            "carddemo.base_batch.jcl_invalid",
            "app/jcl/OPENFIL.jcl is not UTF-8",
        )
    })?;
    let open_id = submit_job_with_retcode(&server, &app, &open_jcl, "CC 0000").await?;
    job_ids.insert("OPENFIL".into(), open_id.clone());
    let mut opened_states = Vec::new();
    for file in ["TRANSACT", "CCXREF", "ACCTDAT", "CXACAIX", "USRSEC"] {
        let state = server
            .cics_service()
            .file_status(file)
            .map_err(terminal_problem)?;
        opened_states.push((file, state));
        if state != CicsFileStatus::Open {
            return Err(CorpusProblem::new(
                "carddemo.base_batch.cics_open_drift",
                format!("{file} was not opened by OPENFIL"),
            ));
        }
    }
    let (opened_output, opened_more) = server
        .batch_service()
        .spool(&control_invocation, &open_id, "CMDOUT", 0, 16)
        .map_err(terminal_problem)?;
    if opened_more
        || opened_output
            != [
                "TRANSACT OPEN",
                "CCXREF OPEN",
                "ACCTDAT OPEN",
                "CXACAIX OPEN",
                "USRSEC OPEN",
            ]
            .map(|record| record.as_bytes().to_vec())
    {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.cics_open_drift",
            "OPENFIL command output differs",
        ));
    }
    file_controls.compare_opened(
        open_jcl.as_bytes(),
        &opened_states,
        &opened_output,
        opened_more,
    )?;

    let rollback_jcl = "//CD23ROLL JOB CLASS=A\n//FAIL EXEC PGM=NOTREAL\n//WORK DD DSN=AWS.M2.CARDDEMO.CD23.ROLLBACK,DISP=(NEW,KEEP,DELETE),\n// UNIT=SYSDA,DCB=(LRECL=80,RECFM=FB)\n";
    server
        .start_background_workers()
        .map_err(terminal_problem)?;
    let rollback_headers = base_batch_job_headers();
    let (rollback_status, rollback_body) = terminal_http(
        &app,
        Method::PUT,
        "/zosmf/restjobs/jobs",
        rollback_headers.clone(),
        rollback_jcl.as_bytes().to_vec(),
    )
    .await?;
    let rollback_job: serde_json::Value = serde_json::from_slice(&rollback_body)
        .map_err(|error| CorpusProblem::new("carddemo.base_batch.rollback", error.to_string()))?;
    let rollback_job = if rollback_status == StatusCode::CREATED {
        wait_for_submitted_job(&server, &app, &rollback_headers, rollback_job).await?
    } else {
        rollback_job
    };
    if rollback_status != StatusCode::CREATED
        || rollback_job["status"] != "OUTPUT"
        || !rollback_job["retcode"].is_null()
        || server.dataset_service().invoke(DatasetRequest::Attributes {
            dataset: DatasetName::new(
                "AWS.M2.CARDDEMO.CD23.ROLLBACK",
                InvocationLimits::default().max_binding_bytes,
            )
            .expect("static rollback dataset"),
        }) != Err(HostProblem::NotFound)
    {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.rollback",
            format!("abnormal allocation was not rolled back: {rollback_job}"),
        ));
    }
    job_ids.insert(
        "ROLLBACK-CONTROL".into(),
        rollback_job["jobid"]
            .as_str()
            .ok_or_else(|| {
                CorpusProblem::new("carddemo.base_batch.rollback", "rollback job ID is missing")
            })?
            .to_string(),
    );

    let cancelled = server
        .batch_service()
        .submit(
            &control_invocation,
            &JclBundle {
                primary: "//CD23CANC JOB CLASS=A\n//WAIT EXEC PGM=IEFBR14\n".into(),
                ..Default::default()
            },
            &IdempotencyKey::new("carddemo-base-batch-cancel", InvocationLimits::default())
                .expect("static cancellation idempotency key"),
            true,
        )
        .map_err(terminal_problem)?;
    let (cancel_status, _) = terminal_http(
        &app,
        Method::PUT,
        &format!("/zosmf/restjobs/jobs/{}/{}", cancelled.name, cancelled.id),
        base_batch_job_headers(),
        Vec::new(),
    )
    .await?;
    if cancel_status != StatusCode::NO_CONTENT
        || server
            .batch_service()
            .get(&cancelled.id)
            .map_err(terminal_problem)?
            .state
            != JobState::Cancelled
    {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.cancellation",
            "held job did not cancel through the public z/OSMF route",
        ));
    }
    job_ids.insert("CANCEL-CONTROL".into(), cancelled.id);

    financial_checks.seal_before_restart(&server)?;
    let dataset_sha256 = base_batch_dataset_digests(&server)?;
    let spool_sha256 = base_batch_spool_digests(&server, &job_ids)?;
    drop(app);
    if !server.graceful_shutdown().await {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.shutdown_failed",
            "base batch server did not shut down",
        ));
    }
    drop(racf);
    drop(server);
    let restarted = ProductServer::open(config, store, secrets, default_program_router())
        .map_err(terminal_problem)?;
    if base_batch_dataset_digests(&restarted)? != dataset_sha256
        || base_batch_spool_digests(&restarted, &job_ids)? != spool_sha256
        || ["TRANSACT", "CCXREF", "ACCTDAT", "CXACAIX", "USRSEC"]
            .into_iter()
            .any(|file| restarted.cics_service().file_status(file) != Ok(CicsFileStatus::Open))
    {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.restart_drift",
            "dataset, spool, or CICS state changed across warm restart",
        ));
    }
    financial_checks.compare_reopened(&restarted)?;
    if !restarted.graceful_shutdown().await {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.shutdown_failed",
            "restarted base batch server did not shut down",
        ));
    }
    drop(restarted);

    let journeys_passed = 3usize;
    let initialization_jobs = 12usize;
    let operational_jobs = 9usize;
    let cics_file_controls = 2usize;
    let internal_submissions = 1usize;
    let warm_restart_controls = 1usize;
    let rollback_controls = 1usize;
    let cancellation_controls = 1usize;
    let mut shape = Sha256::new();
    digest_field(&mut shape, corpus_commit.as_bytes());
    digest_field(&mut shape, BUSINESS_DATE.as_bytes());
    for value in [
        journeys_passed,
        initialization_jobs,
        operational_jobs,
        cics_file_controls,
        internal_submissions,
        warm_restart_controls,
        rollback_controls,
        cancellation_controls,
        tranrept_selected_records,
        tranrept_report_records,
    ] {
        digest_field(&mut shape, &(value as u64).to_be_bytes());
    }
    for (name, digest) in &dataset_sha256 {
        digest_field(&mut shape, name.as_bytes());
        digest_field(&mut shape, digest.as_bytes());
    }
    for (name, digest) in &spool_sha256 {
        digest_field(&mut shape, name.as_bytes());
        digest_field(&mut shape, digest.as_bytes());
    }
    fs::remove_dir_all(&artifact_root).map_err(|error| {
        CorpusProblem::new("carddemo.base_batch.cleanup_failed", error.to_string())
    })?;
    let mut route_observations = RouteObservations::default();
    file_controls.finish(&mut route_observations)?;
    let financial_proof = financial_checks.finish(&mut route_observations)?;
    Ok((
        CardDemoBaseBatchReceipt {
            schema_version: "mainframe-env.carddemo-base-batch-receipt@1".into(),
            status: "pass".into(),
            corpus_commit,
            business_date: BUSINESS_DATE.into(),
            journeys_passed,
            initialization_jobs,
            operational_jobs,
            cics_file_controls,
            internal_submissions,
            warm_restart_controls,
            rollback_controls,
            cancellation_controls,
            tranrept_selected_records,
            tranrept_report_records,
            dataset_sha256,
            spool_sha256,
            journey_shape_sha256: format!("{:x}", shape.finalize()),
        },
        route_observations,
        financial_proof,
    ))
}

pub(super) fn base_batch_job_label(relative: &str) -> Result<String, CorpusProblem> {
    Path::new(relative)
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_uppercase)
        .ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.base_batch.path_invalid",
                format!("{relative} has no job label"),
            )
        })
}

pub(super) fn base_batch_job_headers() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "authorization".into(),
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode("IBMUSER:TESTPASS")
            ),
        ),
        ("x-csrf-zosmf-header".into(), "true".into()),
    ])
}

pub(super) fn base_batch_control_invocation() -> Result<Invocation, CorpusProblem> {
    let limits = InvocationLimits::default();
    let grants = [
        "host.security.authorize",
        "host.program.invoke",
        "host.spool.read",
        "host.spool.write",
    ]
    .into_iter()
    .map(|capability| {
        CapabilityId::new(capability, limits).map_err(|_| {
            CorpusProblem::new(
                "carddemo.base_batch.control_invalid",
                "control capability is invalid",
            )
        })
    })
    .collect::<Result<BTreeSet<_>, _>>()?;
    Invocation::new(
        RequestId::new("carddemo-base-batch-control-request", limits)
            .expect("static control request"),
        ExecutionId::new("carddemo-base-batch-control-execution", limits)
            .expect("static control execution"),
        RunUnitId::new("carddemo-base-batch-control-run", limits).expect("static control run"),
        None,
        Selector::new("zosmf:job-control", limits).expect("static control selector"),
        ArtifactRef::new("mainframe-env-batch@1", limits).expect("static control artifact"),
        Principal::new(
            PrincipalId::new("IBMUSER", limits).expect("static control principal"),
            grants,
            limits,
        )
        .expect("bounded control principal"),
        ServiceClass::Batch,
        0,
        1_000_000,
        TraceId::new("carddemo-base-batch-control-trace", limits).expect("static control trace"),
        IdempotencyKey::new("carddemo-base-batch-control-invocation", limits)
            .expect("static control invocation key"),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .map_err(|_| {
        CorpusProblem::new(
            "carddemo.base_batch.control_invalid",
            "control invocation is invalid",
        )
    })
}

pub(super) fn base_batch_spool_text(
    server: &ProductServer,
    id: &str,
) -> Result<String, CorpusProblem> {
    let invocation = base_batch_control_invocation()?;
    let mut text = String::new();
    for (_, name, records, _) in server
        .batch_service()
        .spool_files(&invocation, id)
        .map_err(terminal_problem)?
    {
        let (values, more) = server
            .batch_service()
            .spool(&invocation, id, &name, 0, records.max(1))
            .map_err(terminal_problem)?;
        if more {
            return Err(CorpusProblem::new(
                "carddemo.base_batch.spool_limit",
                format!("{id}/{name} exceeded its declared record count"),
            ));
        }
        for value in values {
            text.push_str(&String::from_utf8_lossy(&value));
            text.push('\n');
        }
    }
    Ok(text)
}

pub(super) fn base_batch_dataset_digests(
    server: &ProductServer,
) -> Result<BTreeMap<String, String>, CorpusProblem> {
    let DatasetResult::Listed { names, more } = server
        .dataset_service()
        .invoke(DatasetRequest::List {
            pattern: "AWS.M2.CARD*".into(),
            start: None,
            max_items: 4_096,
        })
        .map_err(terminal_problem)?
    else {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.catalog_drift",
            "dataset list returned the wrong result",
        ));
    };
    if more {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.catalog_limit",
            "base-cycle catalog exceeded 4096 entries",
        ));
    }
    let mut observations = BTreeMap::<String, String>::new();
    let mut catalog = Sha256::new();
    for name in &names {
        digest_field(&mut catalog, name.as_str().as_bytes());
    }
    observations.insert("@CATALOG".into(), format!("{:x}", catalog.finalize()));
    for name in names {
        let attributes = match server.dataset_service().invoke(DatasetRequest::Attributes {
            dataset: name.clone(),
        }) {
            Ok(DatasetResult::Attributes {
                attributes,
                version,
            }) => (attributes, version),
            Err(HostProblem::NotFound) => continue,
            Ok(_) => {
                return Err(CorpusProblem::new(
                    "carddemo.base_batch.dataset_drift",
                    format!("{} attributes returned the wrong result", name.as_str()),
                ));
            }
            Err(problem) => return Err(terminal_problem(problem)),
        };
        let mut digest = Sha256::new();
        digest_field(
            &mut digest,
            format!("{:?}", attributes.0.organization).as_bytes(),
        );
        digest_field(
            &mut digest,
            format!("{:?}", attributes.0.record_format).as_bytes(),
        );
        digest_field(
            &mut digest,
            &attributes.0.logical_record_length.to_be_bytes(),
        );
        digest_field(
            &mut digest,
            &attributes.0.key_offset.unwrap_or(u32::MAX).to_be_bytes(),
        );
        digest_field(
            &mut digest,
            &attributes.0.key_length.unwrap_or(u32::MAX).to_be_bytes(),
        );
        digest_field(
            &mut digest,
            &attributes.0.ccsid.unwrap_or(u16::MAX).to_be_bytes(),
        );
        digest_field(&mut digest, &attributes.1.to_be_bytes());
        if attributes.0.organization == DatasetOrganization::Partitioned {
            let DatasetResult::Members {
                names: members,
                more,
            } = server
                .dataset_service()
                .invoke(DatasetRequest::ListMembers {
                    dataset: name.clone(),
                    start: None,
                    max_items: 4_096,
                })
                .map_err(terminal_problem)?
            else {
                return Err(CorpusProblem::new(
                    "carddemo.base_batch.member_drift",
                    format!("{} member list returned the wrong result", name.as_str()),
                ));
            };
            if more {
                return Err(CorpusProblem::new(
                    "carddemo.base_batch.member_limit",
                    format!("{} exceeded 4096 members", name.as_str()),
                ));
            }
            for member in members {
                digest_field(&mut digest, member.as_str().as_bytes());
                let DatasetResult::Records {
                    records,
                    identities,
                    version,
                } = server
                    .dataset_service()
                    .invoke(DatasetRequest::Read {
                        dataset: name.clone(),
                        member: Some(member),
                        key: None,
                        max_records: 4_096,
                        control: Default::default(),
                    })
                    .map_err(terminal_problem)?
                else {
                    return Err(CorpusProblem::new(
                        "carddemo.base_batch.member_drift",
                        format!("{} member read returned the wrong result", name.as_str()),
                    ));
                };
                digest_field(&mut digest, &version.to_be_bytes());
                for record in records {
                    digest_field(&mut digest, &record);
                }
                for identity in identities {
                    digest_field(&mut digest, &identity);
                }
            }
        } else {
            let DatasetResult::Records {
                records,
                identities,
                version,
            } = server
                .dataset_service()
                .invoke(DatasetRequest::Read {
                    dataset: name.clone(),
                    member: None,
                    key: None,
                    max_records: 4_096,
                    control: Default::default(),
                })
                .map_err(terminal_problem)?
            else {
                return Err(CorpusProblem::new(
                    "carddemo.base_batch.dataset_drift",
                    format!("{} read returned the wrong result", name.as_str()),
                ));
            };
            digest_field(&mut digest, &version.to_be_bytes());
            for record in records {
                digest_field(&mut digest, &record);
            }
            for identity in identities {
                digest_field(&mut digest, &identity);
            }
        }
        observations.insert(name.as_str().into(), format!("{:x}", digest.finalize()));
    }
    for required in [
        "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS",
        "AWS.M2.CARDDEMO.CARDXREF.VSAM.AIX.PATH",
        "AWS.M2.CARDDEMO.TRANSACT.VSAM.AIX.PATH",
        "AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS",
        "AWS.M2.CARDDEMO.TRXFL.SEQ",
        "AWS.M2.CARDDEMO.TRXFL.VSAM.KSDS",
        "AWS.M2.CARDDEMO.STATEMNT.PS",
        "AWS.M2.CARDDEMO.STATEMNT.HTML",
        "AWS.M2.CARDDEMO.TCATBALF.REPT",
        "AWS.M2.CARDEMO.FTP.TEST.BKUP.INTRDR",
    ] {
        if !observations.contains_key(required) {
            return Err(CorpusProblem::new(
                "carddemo.base_batch.dataset_missing",
                format!("{required} is missing from exact observations"),
            ));
        }
    }
    for base in [
        "AWS.M2.CARDDEMO.DALYREJS.G",
        "AWS.M2.CARDDEMO.SYSTRAN.G",
        "AWS.M2.CARDDEMO.TRANSACT.BKUP.G",
        "AWS.M2.CARDDEMO.TRANSACT.COMBINED.G",
        "AWS.M2.CARDDEMO.TRANSACT.DALY.G",
        "AWS.M2.CARDDEMO.TRANREPT.G",
        "AWS.M2.CARDDEMO.TCATBALF.BKUP.G",
    ] {
        if !observations.keys().any(|name| name.starts_with(base)) {
            return Err(CorpusProblem::new(
                "carddemo.base_batch.gdg_missing",
                format!("{base} generation is missing"),
            ));
        }
    }
    Ok(observations)
}

pub(super) fn base_batch_spool_digests(
    server: &ProductServer,
    job_ids: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, CorpusProblem> {
    let invocation = base_batch_control_invocation()?;
    let mut observations = BTreeMap::new();
    for (label, id) in job_ids {
        let job = server.batch_service().get(id).map_err(terminal_problem)?;
        if !job.state.terminal() {
            return Err(CorpusProblem::new(
                "carddemo.base_batch.job_incomplete",
                format!("{label}/{id} is not terminal"),
            ));
        }
        let mut digest = Sha256::new();
        digest_field(&mut digest, label.as_bytes());
        digest_field(&mut digest, id.as_bytes());
        digest_field(&mut digest, format!("{:?}", job.state).as_bytes());
        digest_field(
            &mut digest,
            &job.return_code.unwrap_or(i32::MIN).to_be_bytes(),
        );
        digest_field(
            &mut digest,
            job.abend_code.as_deref().unwrap_or("").as_bytes(),
        );
        for (_, name, records, _) in server
            .batch_service()
            .spool_files(&invocation, id)
            .map_err(terminal_problem)?
        {
            digest_field(&mut digest, name.as_bytes());
            let (values, more) = server
                .batch_service()
                .spool(&invocation, id, &name, 0, records.max(1))
                .map_err(terminal_problem)?;
            if more {
                return Err(CorpusProblem::new(
                    "carddemo.base_batch.spool_limit",
                    format!("{label}/{name} exceeded its declared record count"),
                ));
            }
            for value in values {
                digest_field(&mut digest, &value);
            }
        }
        observations.insert(label.clone(), format!("{:x}", digest.finalize()));
    }
    Ok(observations)
}
