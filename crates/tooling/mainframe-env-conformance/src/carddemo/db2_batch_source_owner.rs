//! Distinct batch-only first-party Db2 owner; original public99 is never offered.
//! Existing complete local comparators and private retention handle are reused.
//! Same MemoryStore reopen; no physical SQLite, application rollback or IBM credit.
use super::*;

fn batch_refused(detail: &str) -> CorpusProblem {
    CorpusProblem::new("carddemo.db2.batch_source_owner", detail)
}

fn compile_batch(corpus: &Path) -> Result<Vec<BatchProgramDefinition>, CorpusProblem> {
    let catalog = carddemo_db2_bundles(corpus)?
        .into_iter()
        .map(|(relative, bundle)| {
            let name = Path::new(&relative)
                .file_stem()
                .and_then(|name| name.to_str())
                .ok_or_else(|| batch_refused("batch source name invalid"))?
                .to_ascii_uppercase();
            Ok((name, (relative, bundle)))
        })
        .collect::<Result<BTreeMap<_, _>, CorpusProblem>>()?;
    compile_carddemo_batch_definitions(&catalog, &BTreeSet::from(["COBTUPDT".into()]))
}

fn seed_inputs(server: Arc<ProductServer>, corpus_dir: &Path) -> Result<(), CorpusProblem> {
    let mut sequence = 40_000u64;
    let mut control_members = Vec::new();
    for relative in collect_paths(corpus_dir, &["app/app-transaction-type-db2/ctl"], "ctl")? {
        let member = Path::new(&relative)
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                CorpusProblem::new("carddemo.db2.control_invalid", "control member is invalid")
            })?;
        let source = String::from_utf8(read_corpus_file(corpus_dir, &corpus_dir.join(&relative))?)
            .map_err(|_| {
                CorpusProblem::new(
                    "carddemo.db2.control_invalid",
                    format!("{relative} is not UTF-8"),
                )
            })?;
        control_members.push((
            member.to_string(),
            source
                .lines()
                .map(|line| line.as_bytes().to_vec())
                .collect(),
        ));
    }
    control_library::seed_control_library(
        &server,
        "AWS.M2.CARDDEMO.CNTL",
        control_members,
        &mut sequence,
    )?;
    utility_seed_dataset(
        &server,
        "INPFILE",
        DatasetOrganization::Sequential,
        RecordFormat::Fixed,
        53,
        None,
        ["A98BATCH INSERT", "U02BATCH PAYMENT", "D98"]
            .into_iter()
            .map(|record| {
                let mut record = record.as_bytes().to_vec();
                record.resize(53, b' ');
                record
            })
            .collect(),
        &mut sequence,
    )?;
    for dataset in ["AWS.M2.CARDDEMO.TRANTYPE.PS", "AWS.M2.CARDDEMO.TRANCATG.PS"] {
        utility_seed_dataset(
            &server,
            dataset,
            DatasetOrganization::Sequential,
            RecordFormat::Fixed,
            60,
            None,
            vec![vec![b' '; 60]],
            &mut sequence,
        )?;
    }
    for base in [
        "AWS.M2.CARDDEMO.TRANTYPE.BKUP",
        "AWS.M2.CARDDEMO.TRANCATG.PS.BKUP",
    ] {
        server
            .dataset_service()
            .invoke(DatasetRequest::DefineGenerationGroup {
                base: DatasetName::new(base, 128).expect("static Db2 GDG base"),
                limit: 5,
                scratch: true,
                empty: false,
                mutation: Mutation {
                    sequence,
                    idempotency_key: IdempotencyKey::new(
                        format!("carddemo-db2-gdg-{sequence}"),
                        InvocationLimits::default(),
                    )
                    .expect("bounded Db2 GDG mutation"),
                    transaction: Some("CD-024".into()),
                },
            })
            .map_err(terminal_problem)?;
        sequence += 1;
    }

    Ok(())
}

async fn exercise(
    corpus_dir: &Path,
    owned: &mut ReceiptOwner,
    receipt: &mut serde_json::Value,
) -> Result<(), CorpusProblem> {
    owned.check()?;
    let config = ServerConfig {
        store_profile: StoreProfile::Memory,
        artifact_root: owned.artifacts.clone(),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    let store = Arc::new(MemoryStore::new(Default::default()));
    let secrets = Arc::new(MemorySecretResolver::default());
    let server = ProductServer::open_with_package_trust(
        config.clone(),
        store.clone(),
        secrets.clone(),
        default_program_router(),
        carddemo_package_trust()?,
    )
    .map_err(terminal_problem)?;
    let app = server.router();
    let mut job_ids = BTreeMap::new();
    let mut maintenance_before = None;
    let mut before_reopen = None;
    let probe = async {
        owned.check()?;
        receipt["stage"] = serde_json::json!("compile-install");
        owned.save(receipt)?;
        db2_local_observations::check_inputs(corpus_dir)?;
        db2_control_observations::check_inputs(corpus_dir)?;
        let definitions = compile_batch(corpus_dir)?;
        if definitions.len() != 1 || definitions[0].name != "COBTUPDT" {
            return Err(batch_refused(
                "one selected native COBTUPDT definition required",
            ));
        }
        receipt["compiled_batch_artifact"] = serde_json::json!({
            "name": definitions[0].name,
            "artifact": definitions[0].artifact.as_str(),
            "payload_sha256": format!("{:x}", Sha256::digest(&definitions[0].payload)),
            "semantic_identity": definitions[0].semantic_identity,
        });
        owned.check()?;
        server
            .install_batch_programs(definitions.clone())
            .map_err(terminal_problem)?;
        install_carddemo_db2_package(&server, &definitions)?;
        db2_local_observations::require_state(&server, db2_local_observations::RowsPhase::Empty)?;
        server
            .bootstrap_user("IBMUSER", b"TESTPASS")
            .map_err(terminal_problem)?;
        let racf = server.racf_service();
        // Exact declared batch principal only; no terminal/admin offers.
        for table in [
            "CARDDEMO.TRANSACTION_TYPE",
            "CARDDEMO.TRANSACTION_TYPE_CATEGORY",
            "SYSIBM.SYSDUMMY1",
        ] {
            racf.define_profile("DB2TABLE", table, "IBMUSER", None)
                .map_err(terminal_problem)?;
            racf.permit("DB2TABLE", table, "IBMUSER", AccessIntent::Alter)
                .map_err(terminal_problem)?;
        }
        for dataset in ["AWS.M2.CARDDEMO.**", "INPFILE"] {
            racf.define_profile("DATASET", dataset, "IBMUSER", None)
                .map_err(terminal_problem)?;
            racf.permit("DATASET", dataset, "IBMUSER", AccessIntent::Alter)
                .map_err(terminal_problem)?;
        }
        owned.check()?;
        receipt["stage"] = serde_json::json!("source-controls-and-seeds");
        owned.save(receipt)?;
        seed_inputs(server.clone(), corpus_dir)?;
        owned.check()?;
        receipt["stage"] = serde_json::json!("create-load");
        owned.save(receipt)?;
        let create = String::from_utf8(read_corpus_file(
            corpus_dir,
            &corpus_dir.join("app/app-transaction-type-db2/jcl/CREADB21.jcl"),
        )?)
        .map_err(|_| CorpusProblem::new("carddemo.db2.jcl_invalid", "CREADB21 is not UTF-8"))?;
        db2_local_observations::require_controls(&server, corpus_dir)?;
        let create_id = submit_job_with_retcode(&server, &app, &create, "CC 0000").await?;
        db2_local_observations::require_utility(
            &server,
            &create_id,
            db2_local_observations::UtilityJob::Create,
        )?;
        db2_local_observations::require_state(&server, db2_local_observations::RowsPhase::Loaded)?;
        job_ids.insert("CREADB2".into(), create_id);
        if server
            .db2_service()
            .table_rows("CARDDEMO.TRANSACTION_TYPE")
            .map_err(terminal_problem)?
            .len()
            != 7
            || server
                .db2_service()
                .table_rows("CARDDEMO.TRANSACTION_TYPE_CATEGORY")
                .map_err(terminal_problem)?
                .len()
                != 18
        {
            return Err(CorpusProblem::new(
                "carddemo.db2.load_drift",
                "CREADB21 did not create and load 7 type plus 18 category rows",
            ));
        }

        owned.check()?;
        // Existing typed control requests under this fixture's admitted batch principal.
        // This direct control earns no application DML/commit/rollback credit.
        let mut left = db2_control_invocation("db2-conflict-left")?;
        let invocation_limits = InvocationLimits::default();
        left.principal = Principal::new(
            PrincipalId::new("IBMUSER", invocation_limits)
                .map_err(|_| batch_refused("declared batch principal invalid"))?,
            left.principal.grants().clone(),
            invocation_limits,
        )
        .map_err(|_| batch_refused("existing control grants invalid"))?;
        let updated = server
            .db2_service()
            .execute(
                &left,
                &db2_control_request(
                    Db2Operation::Update,
                    50_003,
                    BTreeMap::from([
                        ("DCL-TR-TYPE".into(), db2_variable("02")),
                        ("DCL-TR-DESCRIPTION".into(), db2_varchar_variable("LEFT")),
                    ]),
                )?,
            )
            .map_err(terminal_problem)?;
        let committed = server
            .db2_service()
            .execute(
                &left,
                &db2_control_request(Db2Operation::Commit, 50_005, BTreeMap::new())?,
            )
            .map_err(terminal_problem)?;
        if updated.sqlcode != 0 || committed.sqlcode != 0 {
            return Err(batch_refused("actual typed LEFT preparation refused"));
        }
        maintenance_before = Some(db2_control_observations::require_maintenance_before(
            &server,
        )?);
        receipt["stage"] = serde_json::json!("native-maintenance");
        owned.save(receipt)?;
        // The existing comparator's local container is discarded. No journey
        // receipt, issue closure or new acceptance token is returned here.
        let mut route_observations = RouteObservations::default();
        let maintenance = String::from_utf8(read_corpus_file(
            corpus_dir,
            &corpus_dir.join("app/app-transaction-type-db2/jcl/MNTTRDB2.jcl"),
        )?)
        .map_err(|_| CorpusProblem::new("carddemo.db2.jcl_invalid", "MNTTRDB2 is not UTF-8"))?;
        let maintenance_id =
            submit_job_with_retcode(&server, &app, &maintenance, "CC 0000").await?;
        job_ids.insert("MNTTRDB2".into(), maintenance_id.clone());
        owned.check()?;
        let maintained_rows = server
            .db2_service()
            .table_rows("CARDDEMO.TRANSACTION_TYPE")
            .map_err(terminal_problem)?;
        if maintained_rows
            .iter()
            .any(|row| row.first().is_some_and(|value| value == b"98"))
            || !maintained_rows.iter().any(|row| {
                row.first().is_some_and(|value| value == b"02")
                    && row
                        .get(1)
                        .is_some_and(|value| value.as_slice() == b"BATCH PAYMENT")
            })
        {
            return Err(CorpusProblem::new(
                "carddemo.db2.batch_maintenance_drift",
                format!(
                    "COBTUPDT did not apply insert, update, and delete commands: {:?}",
                    maintained_rows
                        .iter()
                        .map(|row| row
                            .iter()
                            .map(|column| String::from_utf8_lossy(column).to_string())
                            .collect::<Vec<_>>())
                        .collect::<Vec<_>>()
                ),
            ));
        }
        db2_control_observations::compare_completed_maintenance(
            &server,
            &maintenance_id,
            maintenance_before
                .as_ref()
                .ok_or_else(|| batch_refused("before state absent"))?,
            &mut route_observations,
        )?;
        receipt["stage"] = serde_json::json!("extract");
        owned.save(receipt)?;
        let extract = String::from_utf8(read_corpus_file(
            corpus_dir,
            &corpus_dir.join("app/app-transaction-type-db2/jcl/TRANEXTR.jcl"),
        )?)
        .map_err(|_| CorpusProblem::new("carddemo.db2.jcl_invalid", "TRANEXTR is not UTF-8"))?;
        let extract_id = submit_job_with_retcode(&server, &app, &extract, "CC 0000").await?;
        db2_local_observations::require_utility(
            &server,
            &extract_id,
            db2_local_observations::UtilityJob::Extract,
        )?;
        db2_local_observations::require_state(
            &server,
            db2_local_observations::RowsPhase::Maintained,
        )?;
        job_ids.insert("TRANEXTR".into(), extract_id);

        let extracted_type = utility_records(&server, "AWS.M2.CARDDEMO.TRANTYPE.PS", None)?;
        let extracted_category = utility_records(&server, "AWS.M2.CARDDEMO.TRANCATG.PS", None)?;
        let extraction_records = extracted_type.len() + extracted_category.len();
        if extraction_records < 25
            || extracted_type.iter().any(|record| record.len() != 60)
            || extracted_category.iter().any(|record| record.len() != 60)
        {
            return Err(CorpusProblem::new(
                "carddemo.db2.extract_drift",
                "DSNTIAUL extraction did not produce exact 60-byte type/category records",
            ));
        }

        db2_observations::require_extracted(&extracted_type, &extracted_category)?;
        receipt["stage"] = serde_json::json!("vsam-import");
        owned.save(receipt)?;
        for name in ["TRANTYPE", "TRANCATG"] {
            owned.check()?;
            let relative = format!("app/jcl/{name}.jcl");
            let import =
                String::from_utf8(read_corpus_file(corpus_dir, &corpus_dir.join(&relative))?)
                    .map_err(|_| {
                        CorpusProblem::new(
                            "carddemo.db2.jcl_invalid",
                            "VSAM import JCL is not UTF-8",
                        )
                    })?;
            let id = submit_job_with_retcode(&server, &app, &import, "CC 0000").await?;
            job_ids.insert(name.into(), id);
        }
        db2_observations::require_vsam(&server)?;

        owned.check()?;
        if job_ids.len() != 5 || extraction_records != 25 {
            return Err(batch_refused(
                "five source jobs and twenty-five exact records required",
            ));
        }
        before_reopen = Some((
            db2_table_digests(&server)?,
            db2_dataset_digests(&server)?,
            base_batch_spool_digests(&server, &job_ids)?,
        ));
        receipt["job_ids"] = serde_json::json!(&job_ids);
        receipt["extraction_records"] = serde_json::json!(extraction_records);
        receipt["stage"] = serde_json::json!("before-first-drain");
        Ok::<_, CorpusProblem>(())
    }
    .await;
    if let Err(problem) = &probe {
        receipt["problem_code"] = serde_json::json!(problem.code);
    }
    let saved_before_drain = owned.save(receipt);
    drop(app);
    let drained = server.graceful_shutdown().await;
    receipt["first_drain"] = serde_json::json!(drained);
    drop(server);
    let saved_after_drain = owned.save(receipt);
    probe?;
    saved_before_drain?;
    saved_after_drain?;
    if !drained {
        return Err(batch_refused("first drain refused; preserve artifacts"));
    }
    owned.check()?;
    receipt["stage"] = serde_json::json!("same-memory-store-reopen");
    owned.save(receipt)?;
    let restarted = ProductServer::open_with_package_trust(
        config,
        store.clone(),
        secrets,
        default_program_router(),
        carddemo_package_trust()?,
    )
    .map_err(terminal_problem)?;
    let reopened = (|| {
        owned.check()?;
        let actual = (
            db2_table_digests(&restarted)?,
            db2_dataset_digests(&restarted)?,
            base_batch_spool_digests(&restarted, &job_ids)?,
        );
        if Some(&actual) != before_reopen.as_ref() {
            return Err(batch_refused(
                "full table, dataset or spool state changed across reopen",
            ));
        }
        db2_local_observations::require_state(
            &restarted,
            db2_local_observations::RowsPhase::Maintained,
        )?;
        db2_local_observations::require_controls(&restarted, corpus_dir)?;
        db2_local_observations::require_utility(
            &restarted,
            &job_ids["CREADB2"],
            db2_local_observations::UtilityJob::Create,
        )?;
        db2_local_observations::require_utility(
            &restarted,
            &job_ids["TRANEXTR"],
            db2_local_observations::UtilityJob::Extract,
        )?;
        let mut local = RouteObservations::default();
        db2_control_observations::compare_completed_maintenance(
            &restarted,
            &job_ids["MNTTRDB2"],
            maintenance_before
                .as_ref()
                .ok_or_else(|| batch_refused("before state absent"))?,
            &mut local,
        )?;
        db2_observations::require_extracted(
            &utility_records(&restarted, "AWS.M2.CARDDEMO.TRANTYPE.PS", None)?,
            &utility_records(&restarted, "AWS.M2.CARDDEMO.TRANCATG.PS", None)?,
        )?;
        db2_observations::require_vsam(&restarted)?;
        receipt["table_sha256"] = serde_json::json!(actual.0);
        receipt["dataset_sha256"] = serde_json::json!(actual.1);
        receipt["spool_sha256"] = serde_json::json!(actual.2);
        receipt["reopen_compared"] = serde_json::json!(true);
        Ok::<_, CorpusProblem>(())
    })();
    if let Err(problem) = &reopened {
        receipt["problem_code"] = serde_json::json!(problem.code);
    }
    let saved_before_second = owned.save(receipt);
    let second_drain = restarted.graceful_shutdown().await;
    receipt["second_drain"] = serde_json::json!(second_drain);
    drop(restarted);
    drop(store);
    let saved_after_second = owned.save(receipt);
    reopened?;
    saved_before_second?;
    saved_after_second?;
    if !second_drain {
        return Err(batch_refused("second drain refused; preserve artifacts"));
    }
    owned.check()?;
    receipt["stage"] = serde_json::json!("checks-and-two-drains-complete-before-disposal");
    owned.save(receipt)?;
    // Serial exact-owner disposal. Path observations are not an atomic hostile
    // filesystem containment guarantee; no cleanup occurs on prior refusal.
    owned.check()?;
    fs::remove_dir_all(&owned.artifacts).map_err(|_| batch_refused("artifact cleanup failed"))?;
    match fs::symlink_metadata(&owned.artifacts) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        _ => return Err(batch_refused("cleanup absence changed or unknown")),
    }
    owned.artifacts_removed = true;
    owned.check()?;
    receipt["cleanup_verified"] = serde_json::json!(true);
    receipt["stage"] = serde_json::json!("local-source-batch-comparisons-pass");
    owned.save(receipt)
}

#[test]
#[ignore = "distinct batch-only source owner; exact CARDDEMO_CORPUS_DIR and fresh CARDDEMO_DB2_BATCH_EVIDENCE required; no public99/98 or acceptance credit"]
fn pinned_corpus_batch_db2_create_maintenance_extract_reopens_exact_state() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let corpus = verify_carddemo_corpus_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .unwrap();
    let corpus_dir = PathBuf::from(env::var_os(CORPUS_ENV).unwrap());
    db2_local_observations::check_inputs(&corpus_dir).unwrap();
    db2_control_observations::check_inputs(&corpus_dir).unwrap();
    let offered = PathBuf::from(
        env::var_os("CARDDEMO_DB2_BATCH_EVIDENCE")
            .expect("fresh external batch evidence path required"),
    );
    let parent = offered.parent().unwrap().canonicalize().unwrap();
    assert!(
        !parent.starts_with(root.canonicalize().unwrap()),
        "evidence must remain outside Git"
    );
    let evidence = parent.join(offered.file_name().unwrap());
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&evidence)
        .expect("evidence path must be fresh and exclusively owned");
    let claim = fs::symlink_metadata(&evidence).unwrap();
    let evidence_identity = (claim.dev(), claim.ino());
    let artifacts = evidence.join("owned-artifacts");
    require_owned_directory(&evidence, evidence_identity).unwrap();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&artifacts)
        .unwrap();
    let claim = fs::symlink_metadata(&artifacts).unwrap();
    let artifact_identity = (claim.dev(), claim.ino());
    let mut owned =
        ReceiptOwner::open(&evidence, evidence_identity, &artifacts, artifact_identity).unwrap();
    let mut receipt = serde_json::json!({
        "schema":"external.carddemo-db2-batch-source-owner@1",
        "corpus_commit":corpus.commit, "backend":"memory", "physical_store_reopen":false,
        "source_jobs":["CREADB21","MNTTRDB2","TRANEXTR","TRANTYPE","TRANCATG"],
        "transport_actor":"IBMUSER", "native_program":"COBTUPDT",
        "stage":"before-open", "first_drain":false, "second_drain":false,
        "reopen_compared":false, "cleanup_verified":false,
        "application_rollback_credit":0, "vendor_credit":0, "acceptance_credit":0,
        "whole_memory_retained":false, "original99_owner_unchanged":true,
        "original98_owner_unchanged":true, "artifact_root":artifacts,
        "artifact_identity":artifact_identity,
        "controls":"default limits/config/router, original HTTP and job completion bounds",
    });
    owned.save(&receipt).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = runtime.block_on(exercise(&corpus_dir, &mut owned, &mut receipt));
    if let Err(problem) = &result {
        receipt["producer_problem_code"] = serde_json::json!(problem.code);
    }
    owned.save(&receipt).unwrap();
    // Retain only problem codes and bounded technical digests; helper errors
    // may contain response bodies or rows and are not printed here.
    assert!(
        result.is_ok(),
        "distinct Db2 batch source owner refused; private evidence retained at {}",
        evidence.display()
    );
    assert_eq!(receipt["cleanup_verified"], true);
}
