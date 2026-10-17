//! Distinct source-owned ordinary CTLI SQL postimages; original99/98 unchanged.
//! No raw captured bytes, customer fields or credentials enter outward receipts.
use super::*;
#[path = "db2_copyback_comparison.rs"]
mod comparison;

fn owner_refused(detail: &str) -> CorpusProblem {
    CorpusProblem::new("carddemo.db2.ordinary_sql_source_owner", detail)
}
fn need(value: bool, detail: &str) -> Result<(), CorpusProblem> {
    if value {
        Ok(())
    } else {
        Err(owner_refused(detail))
    }
}
fn seed_controls(server: &ProductServer, corpus: &Path) -> Result<(), CorpusProblem> {
    let mut members = Vec::new();
    for relative in collect_paths(corpus, &["app/app-transaction-type-db2/ctl"], "ctl")? {
        let name = Path::new(&relative)
            .file_stem()
            .and_then(|name| name.to_str())
            .ok_or_else(|| owner_refused("control name differs"))?;
        let source = String::from_utf8(read_corpus_file(corpus, &corpus.join(&relative))?)
            .map_err(|_| owner_refused("control encoding differs"))?;
        members.push((
            name.into(),
            source
                .lines()
                .map(|line| line.as_bytes().to_vec())
                .collect(),
        ));
    }
    control_library::seed_control_library(server, "AWS.M2.CARDDEMO.CNTL", members, &mut 40_000)
}
async fn exercise(
    corpus: &Path,
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
    let mut retained = None;
    let probe = async {
        owned.check()?;
        let manager = server
            .install_db2_copyback_observer()
            .map_err(|_| owner_refused("observer installation refused"))?;
        let (online, menu_expected) = carddemo_db2_online_definition(corpus)?;
        let prepared = comparison::Prepared::new(corpus, &online.programs)?;
        receipt["stage"] = serde_json::json!("source-publisher-and-local-catalog");
        owned.save(receipt)?;
        install_carddemo_db2_package(&server, &[])?;
        server
            .bootstrap_user("IBMUSER", b"TESTPASS")
            .map_err(terminal_problem)?;
        install_base_online_authorities(&server, corpus, &online)?;
        online_authorities::install_db2_authorities(&server)?;
        owned.check()?;
        server
            .install_online_application(online)
            .map_err(terminal_problem)?;
        prepared.installed(&owned.artifacts)?;
        let mut menu_proof =
            menu_expected.observe_installed(corpus, store.as_ref(), &owned.artifacts)?;
        let racf = server.racf_service();
        racf.define_profile("DATASET", "AWS.M2.CARDDEMO.**", "IBMUSER", None)
            .map_err(terminal_problem)?;
        racf.permit(
            "DATASET",
            "AWS.M2.CARDDEMO.**",
            "IBMUSER",
            AccessIntent::Alter,
        )
        .map_err(terminal_problem)?;
        seed_controls(&server, corpus)?;
        db2_local_observations::require_controls(&server, corpus)?;
        let jcl = String::from_utf8(read_corpus_file(
            corpus,
            &corpus.join("app/app-transaction-type-db2/jcl/CREADB21.jcl"),
        )?)
        .map_err(|_| owner_refused("seed JCL encoding differs"))?;
        let seed_job = submit_job_with_retcode(&server, &app, &jcl, "CC 0000").await?;
        db2_local_observations::require_utility(
            &server,
            &seed_job,
            db2_local_observations::UtilityJob::Create,
        )?;
        db2_online_rows::require_rows(&server, db2_online_rows::Phase::Seed)?;
        let before_rows = db2_table_digests(&server)?;
        receipt["seed_job"] = serde_json::json!(&seed_job);
        receipt["stage"] = serde_json::json!("known-admin-menu-before-option5");
        owned.save(receipt)?;
        let menu = open_carddemo_menu(
            &server,
            &app,
            "WEBADM",
            "admin-transport-password",
            "ADMIN001",
            "PASSWORD",
            "COADM01",
        )
        .await?;
        require_initial_menu_context(&menu)?;
        let before = db2_admin_menu::before(store.as_ref(), &server, &menu.session)?;
        // Session is genuine and known. Arm before the actual source-selected XCTL;
        // no pending-session endpoint or synthetic actor/checkpoint is introduced.
        let armed = prepared.arm(&manager, &menu.session)?;
        let terminal = carddemo_terminal_exchange(
            &app,
            &menu.session,
            &menu.headers,
            0x7d,
            BTreeMap::from([("OPTION".into(), "5".into())]),
        )
        .await?;
        require_online_mapset(&terminal, "COTRTLI", "ordinary SQL source owner")?;
        menu_proof.observe_selection(
            &db2_admin_menu::Context::new(
                corpus,
                store.as_ref(),
                &server,
                &owned.artifacts,
                &menu.session,
            ),
            &menu.menu_response,
            &terminal,
            before,
            5,
        )?;
        let complete = armed
            .completed()
            .map_err(|_| owner_refused("completed SQL witness refused"))?;
        prepared.compare(
            complete,
            &terminal,
            &menu.session,
            store.as_ref(),
            &owned.artifacts,
        )?;
        need(
            db2_table_digests(&server)? == before_rows,
            "SQL browse changed complete table bytes",
        )?;
        db2_online_rows::require_rows(&server, db2_online_rows::Phase::Seed)?;
        receipt["actual_execution_sha256"] = serde_json::json!(format!(
            "{:x}",
            Sha256::digest(complete.archive().origin().execution().as_str().as_bytes())
        ));
        receipt["actual_run_sha256"] = serde_json::json!(format!(
            "{:x}",
            Sha256::digest(complete.archive().origin().run().as_str().as_bytes())
        ));
        receipt["actual_payload_sha256"] =
            serde_json::json!(hex_metadata(complete.payload_digest()));
        receipt["terminal_version"] = serde_json::json!(complete.terminal_version());
        receipt["terminal_tick"] = serde_json::json!(complete.terminal_tick());
        receipt["actual_statements"] = serde_json::json!(complete.archive().records().len());
        receipt["source_and_storage_compared"] = serde_json::json!(true);
        let uri = format!("/mainframe-env/cics/v1/sessions/{}", menu.session);
        let disconnected =
            terminal_http(&app, Method::DELETE, &uri, menu.headers.clone(), Vec::new()).await?;
        require_terminal_status(
            disconnected.0,
            StatusCode::NO_CONTENT,
            "SQL owner disconnect",
        )?;
        let absent =
            terminal_http(&app, Method::GET, &uri, menu.headers.clone(), Vec::new()).await?;
        require_terminal_status(
            absent.0,
            StatusCode::NOT_FOUND,
            "SQL owner disconnected session",
        )?;
        receipt["session_disconnect_checked"] = serde_json::json!(true);
        retained = Some((prepared, before_rows, seed_job));
        receipt["stage"] = serde_json::json!("before-first-drain");
        Ok::<_, CorpusProblem>(())
    }
    .await;
    if let Err(problem) = &probe {
        receipt["problem_code"] = serde_json::json!(problem.code);
    }
    let saved_before = owned.save(receipt);
    drop(app);
    let drained = server.graceful_shutdown().await;
    receipt["first_drain"] = serde_json::json!(drained);
    drop(server);
    let saved_after = owned.save(receipt);
    saved_before?;
    saved_after?;
    need(drained, "first drain refused; retain artifacts")?;
    owned.check()?;
    receipt["stage"] = serde_json::json!("same-memory-store-reopen");
    owned.save(receipt)?;
    let restarted = ProductServer::open_with_package_trust(
        config,
        store,
        secrets,
        default_program_router(),
        carddemo_package_trust()?,
    )
    .map_err(terminal_problem)?;
    let reopened = (|| {
        owned.check()?;
        let (prepared, rows, seed_job) = retained
            .as_ref()
            .ok_or_else(|| owner_refused("source comparison failed; retain artifacts"))?;
        prepared.installed(&owned.artifacts)?;
        need(
            &db2_table_digests(&restarted)? == rows,
            "full seed rows changed across same-store reopen",
        )?;
        db2_online_rows::require_rows(&restarted, db2_online_rows::Phase::Seed)?;
        db2_local_observations::require_controls(&restarted, corpus)?;
        db2_local_observations::require_utility(
            &restarted,
            seed_job,
            db2_local_observations::UtilityJob::Create,
        )?;
        receipt["reopen_compared"] = serde_json::json!(true);
        Ok::<_, CorpusProblem>(())
    })();
    if let Err(problem) = &reopened {
        receipt["problem_code"] = serde_json::json!(problem.code);
    }
    let saved_before = owned.save(receipt);
    let drained = restarted.graceful_shutdown().await;
    receipt["second_drain"] = serde_json::json!(drained);
    drop(restarted);
    let saved_after = owned.save(receipt);
    probe?;
    reopened?;
    saved_before?;
    saved_after?;
    need(drained, "second drain refused; retain artifacts")?;
    owned.check()?;
    receipt["stage"] = serde_json::json!("checked-storage-and-two-drains-before-disposal");
    owned.save(receipt)?;
    // Serial fixed-owner observation; no hostile filesystem containment or orphan claim.
    owned.check()?;
    fs::remove_dir_all(&owned.artifacts).map_err(|_| owner_refused("artifact cleanup failed"))?;
    match fs::symlink_metadata(&owned.artifacts) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        _ => return Err(owner_refused("artifact absence unknown")),
    }
    owned.artifacts_removed = true;
    owned.check()?;
    receipt["cleanup_verified"] = serde_json::json!(true);
    receipt["stage"] = serde_json::json!("local-source-storage-owner-pass");
    owned.save(receipt)
}
fn hex_metadata(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
#[ignore = "distinct ordinary CTLI source owner; pinned corpus, private frozen SQL oracle dir and fresh evidence required; no99/98/application/vendor credit"]
fn pinned_corpus_admin_menu_ctli_ordinary_sql_storage_has_actual_completed_owner() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let corpus = verify_carddemo_corpus_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .unwrap();
    let corpus_dir = PathBuf::from(env::var_os(CORPUS_ENV).unwrap());
    db2_online_rows::check_sources(&corpus_dir).unwrap();
    db2_local_observations::check_inputs(&corpus_dir).unwrap();
    let offered = PathBuf::from(
        env::var_os("CARDDEMO_DB2_COPYBACK_EVIDENCE")
            .expect("fresh private external SQL evidence required"),
    );
    let parent = offered.parent().unwrap().canonicalize().unwrap();
    assert!(
        !parent.starts_with(root.canonicalize().unwrap()),
        "evidence remains outside Git"
    );
    let evidence = parent.join(offered.file_name().unwrap());
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&evidence)
        .expect("fresh exclusive evidence root required");
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
    let mut receipt = serde_json::json!({"schema":"external.carddemo-ordinary-sql-storage-owner@1", "corpus_commit":corpus.commit,
        "backend":"memory", "physical_store_restart":false, "source_program":"COTRTLIC", "issuing_transaction":"CA00",
        "source_return_transaction":"CTLI", "transport_actor":"WEBADM", "actual_statements":0, "stage":"before-open",
        "first_drain":false, "second_drain":false, "reopen_compared":false, "cleanup_verified":false,
        "source_and_storage_compared":false, "session_disconnect_checked":false, "application_credit":0, "vendor_credit":0,
        "acceptance_credit":0, "original99_owner_unchanged":true, "original98_owner_unchanged":true,
        "raw_storage_retained":false, "artifact_root":artifacts, "artifact_identity":artifact_identity,
        "controls":"default config, clocks, limits, router; original public HTTP/job bounds"});
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
    assert!(
        result.is_ok(),
        "distinct ordinary SQL owner refused; private metadata retained at {}",
        evidence.display()
    );
    assert_eq!(receipt["cleanup_verified"], true);
}
