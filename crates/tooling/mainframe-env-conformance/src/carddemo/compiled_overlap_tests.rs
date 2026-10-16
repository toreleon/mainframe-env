//! A pending actual-input producer, separate from the unchanged physical-backend owner.
use super::overlap_store::ObservedStore;
use super::*;
use std::os::unix::fs::MetadataExt;
use std::time::Duration;

#[path = "compiled_overlap_campaign.rs"]
mod campaign;

#[path = "compiled_overlap_driver_spans.rs"]
mod driver_spans;

#[path = "compiled_overlap_coordination.rs"]
mod coordination;

const SHARED: [&str; 7] = [
    "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS",
    "AWS.M2.CARDDEMO.CUSTDATA.VSAM.KSDS",
    "AWS.M2.CARDDEMO.CARDDATA.VSAM.KSDS",
    "AWS.M2.CARDDEMO.CARDDATA.VSAM.AIX.PATH",
    "AWS.M2.CARDDEMO.CARDXREF.VSAM.KSDS",
    "AWS.M2.CARDDEMO.CARDXREF.VSAM.AIX.PATH",
    "AWS.M2.CARDDEMO.USRSEC.VSAM.KSDS",
];

fn json_problem(error: serde_json::Error) -> CorpusProblem {
    CorpusProblem::new(
        "carddemo.full.compiled_overlap_retention",
        error.to_string(),
    )
}

fn require(ok: bool, message: &str) -> Result<(), CorpusProblem> {
    if ok {
        Ok(())
    } else {
        Err(CorpusProblem::new(
            "carddemo.full.compiled_overlap_drift",
            message,
        ))
    }
}

fn shared_state(server: &ProductServer) -> Result<serde_json::Value, CorpusProblem> {
    let mut rows = Vec::new();
    for name in SHARED {
        let dataset = DatasetName::new(name, 128).map_err(|_| {
            CorpusProblem::new(
                "carddemo.full.compiled_overlap_drift",
                "invalid fixed dataset",
            )
        })?;
        let attributes = server
            .dataset_service()
            .invoke(DatasetRequest::Attributes {
                dataset: dataset.clone(),
            })
            .map_err(terminal_problem)?;
        let records = server
            .dataset_service()
            .invoke(DatasetRequest::Read {
                dataset,
                member: None,
                key: None,
                max_records: 51,
                control: Default::default(),
            })
            .map_err(terminal_problem)?;
        let DatasetResult::Attributes {
            attributes,
            version,
        } = attributes
        else {
            return Err(terminal_problem(HostProblem::Malformed));
        };
        let DatasetResult::Records {
            records,
            identities,
            version: read_version,
        } = records
        else {
            return Err(terminal_problem(HostProblem::Malformed));
        };
        require(
            records.len() <= 50 && records.len() == identities.len() && version == read_version,
            "bounded full shared rows/identities/version differ",
        )?;
        rows.push(serde_json::json!({"dataset":name, "attributes":attributes, "version":version, "records":records, "identities":identities}));
    }
    Ok(serde_json::Value::Array(rows))
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SpoolRecordExpected {
    ordinal: usize,
    bytes: usize,
    sha256: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SpoolOracle {
    schema: String,
    records: Vec<SpoolRecordExpected>,
}

fn compare_spool_records(
    records: &[Vec<u8>],
    more: bool,
    expected: &[SpoolRecordExpected],
) -> Result<(), CorpusProblem> {
    require(
        !more && records.len() == expected.len(),
        "READCUST complete ordered spool cardinality differs",
    )?;
    for (ordinal, (actual, expected)) in records.iter().zip(expected).enumerate() {
        require(
            expected.ordinal == ordinal
                && actual.len() == expected.bytes
                && format!("{:x}", Sha256::digest(actual)) == expected.sha256,
            "READCUST full ordered record length or independent SHA256 differs",
        )?;
    }
    Ok(())
}

fn customer_spool(
    server: &ProductServer,
    id: &str,
    expected: &[SpoolRecordExpected],
) -> Result<Vec<Vec<u8>>, CorpusProblem> {
    let invocation = base_batch_control_invocation()?;
    let (records, more) = server
        .batch_service()
        .spool(&invocation, id, "SYSPRINT", 0, 102)
        .map_err(terminal_problem)?;
    compare_spool_records(&records, more, expected)?;
    Ok(records)
}

async fn session_state(
    app: &axum::Router,
    session: &CardDemoOnlineSession,
) -> Result<serde_json::Value, CorpusProblem> {
    let (status, body) = terminal_http(
        app,
        Method::GET,
        &format!("/mainframe-env/cics/v1/sessions/{}", session.session),
        session.headers.clone(),
        Vec::new(),
    )
    .await?;
    require_terminal_status(status, StatusCode::OK, "compiled mixed session read-back")?;
    serde_json::from_slice(&body).map_err(json_problem)
}

#[test]
#[ignore = "requires exact clean CARDDEMO_CORPUS_DIR and fresh external CARDDEMO_FULL_CONCURRENCY_EVIDENCE"]
fn pinned_corpus_compiled_online_and_batch_overlap() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let inventory = root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json");
    let corpus = verify_carddemo_corpus_from_env(&inventory).unwrap();
    let corpus_dir = PathBuf::from(env::var_os(CORPUS_ENV).unwrap());
    let evidence = PathBuf::from(
        env::var_os("CARDDEMO_FULL_CONCURRENCY_EVIDENCE")
            .expect("fresh external producer directory required"),
    );
    fs::create_dir(&evidence).expect("never reuse an existing producer evidence namespace");
    let online = carddemo_base_online_definition(&corpus_dir).unwrap();
    let online_artifact = online
        .programs
        .iter()
        .find(|definition| definition.name == "COACTVWC")
        .unwrap()
        .artifact
        .clone();
    let bundles = explicit_carddemo_bundles(&corpus_dir)
        .unwrap()
        .into_iter()
        .filter(|(path, _)| path == "app/cbl/CBCUS01C.cbl")
        .map(|(path, source)| ("CBCUS01C".into(), (path, source)))
        .collect();
    let definitions =
        compile_carddemo_batch_definitions(&bundles, &BTreeSet::from(["CBCUS01C".into()])).unwrap();
    let batch_artifact = definitions[0].artifact.clone();
    let jcl = read_corpus_file(&corpus_dir, &corpus_dir.join("app/jcl/READCUST.jcl")).unwrap();
    let input = read_corpus_file(
        &corpus_dir,
        &corpus_dir.join("app/data/EBCDIC/AWS.M2.CARDDEMO.CUSTDATA.PS"),
    )
    .unwrap();
    let (chunks, remainder) = input.as_chunks::<500>();
    assert!(remainder.is_empty() && chunks.len() == 50);
    // Independent CP037-to-ASCII frozen first-party oracle, produced outside
    // the Rust product decoder; metadata hashes every complete ordered DISPLAY
    // record including all padding. Raw expected customer bytes remain external.
    let frozen = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../conformance/subsystems/jes/oracles/carddemo-readcust-sysprint@1.json"
    ));
    assert_eq!(
        format!("{:x}", Sha256::digest(frozen)),
        "8ac5cd40240c64282ce1723ad0ec3cb3b26c389d92a18c7ceb8f0458f8394f23"
    );
    let oracle: SpoolOracle = serde_json::from_slice(frozen).unwrap();
    assert_eq!(
        oracle.schema,
        "mainframe-env.carddemo-readcust-sysprint-oracle@2"
    );
    let expected_spool = oracle.records;
    assert_eq!(expected_spool.len(), 102);
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let artifact_root = env::temp_dir().canonicalize().unwrap().join(format!(
        "mainframe-env-carddemo-compiled-overlap-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&artifact_root).unwrap();
    let claimed = fs::symlink_metadata(&artifact_root).unwrap();
    let claimed_identity = (claimed.dev(), claimed.ino());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut retained = serde_json::json!({
        "schema":"mainframe-env.carddemo-compiled-overlap-development@1", "credit":0,
        "corpus_commit":corpus.commit, "offered_batch_jobs":16, "server_slots":8,
        "oracle_sha256":format!("{:x}", Sha256::digest(frozen)), "jcl_sha256":format!("{:x}", Sha256::digest(&jcl)), "seed_sha256":format!("{:x}", Sha256::digest(&input)),
        "batch_artifact":batch_artifact.as_str(), "online_artifact":online_artifact.as_str(),
        "artifact_root":artifact_root, "artifact_identity":claimed_identity, "server_opened":false, "cleanup_verified":false,
        "controls":"unmodified default program router clock, cancellation and deadline source; original 30-second HTTP and 120-second completion bounds",
    });
    fs::write(
        evidence.join("actual.json"),
        serde_json::to_vec_pretty(&retained).unwrap(),
    )
    .unwrap();
    let result = runtime.block_on(async {
        let driver_spans = Arc::new(driver_spans::DriverSpans::new());
        let observed = Arc::new(ObservedStore::new(Arc::new(MemoryStore::new(Default::default()))));
        let coordination = coordination::Coordination::new(batch_artifact.clone(), online_artifact.clone());
        let scheduled_store = Arc::new(coordination::CoordinatedStore::new(observed.clone(), coordination.clone()));
        let server = ProductServer::open(ServerConfig {
            store_profile: StoreProfile::Memory, artifact_root: artifact_root.clone(), max_concurrency: 8,
            cobol_current_date: Some("2022070600000000+0000".into()),
            tls: TlsConfig { enabled: false, certificate_path: None, private_key_reference: None },
            ..ServerConfig::default()
        }, scheduled_store, Arc::new(MemorySecretResolver::default()), default_program_router()).map_err(terminal_problem)?;
        retained["server_opened"] = serde_json::json!(true);
        let app = server.router();
        let probe = async {
            server.bootstrap_user("IBMUSER", b"TESTPASS").map_err(terminal_problem)?;
            install_base_online_authorities(&server, &corpus_dir, &online)?;
            server.install_online_application(online).map_err(terminal_problem)?;
            server.install_batch_programs(definitions).map_err(terminal_problem)?;
            server.racf_service().define_profile("DATASET", "AWS.M2.CARDDEMO.**", "IBMUSER", None).map_err(terminal_problem)?;
            server.racf_service().permit("DATASET", "AWS.M2.CARDDEMO.**", "IBMUSER", AccessIntent::Read).map_err(terminal_problem)?;
            server.racf_service().permit("DATASET", "AWS.M2.CARDDEMO.CUSTDATA.VSAM.KSDS", "IBMUSER", AccessIntent::Read).map_err(terminal_problem)?;
            let account = select_regular_option(&server, &app, 1, "COACTVW").await?;
            let cards = select_regular_option(&server, &app, 3, "COCRDLI").await?;
            require(account.session != cards.session, "mixed online sessions must have separate identities")?;
            let fields = BTreeMap::from([("ACCTSID".into(), "00000000050".into())]);
            let account_baseline = carddemo_terminal_exchange(&app, &account.session, &account.headers, 0x7d, fields.clone()).await?;
            let cards_baseline = carddemo_terminal_exchange(&app, &cards.session, &cards.headers, 0xf7, BTreeMap::new()).await?;
            let mut standalone_oracle = RouteObservations::default();
            bms_seed_oracles::compare_account_view(&corpus_dir, &account_baseline, &online_screen_fields(&account_baseline)?, &mut standalone_oracle)?;
            require_online_mapset(&cards_baseline, "COCRDLI", "standalone card comparison")?;
            // Stage the exact two INPUT requests before offering the campaign.
            // INPUT updates each existing session; only RESUME runs its actor.
            let (account_input, cards_input) = tokio::join!(
                campaign::stage_input(&app, &account, 0x7d, fields),
                campaign::stage_input(&app, &cards, 0xf7, BTreeMap::new()),
            );
            account_input?; cards_input?;
            let before = shared_state(&server)?;
            retained["before"] = before.clone();
            observed.arm()?;
            let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
            coordination.arm(deadline.into_std())?;
            let _release_before_drain = coordination.release_on_drop();
            let headers = BTreeMap::from([("authorization".into(), format!("Basic {}", base64::engine::general_purpose::STANDARD.encode("IBMUSER:TESTPASS"))), ("x-csrf-zosmf-header".into(), "true".into())]);
            let mut tasks = Vec::new();
            for request_index in 0..16 {
                let route = app.clone(); let headers = headers.clone(); let jcl = jcl.clone();
                tasks.push(tokio::spawn(coordination.clone().guard_result(driver_spans.observe(request_index, async move { terminal_http(&route, Method::PUT, "/zosmf/restjobs/jobs", headers, jcl).await }))));
            }
            // Explicitly coordinate a real effect boundary. Passive capture,
            // original requests and the completed-effect witness are unchanged.
            let exchanges = async {
                let _release_before_joins = coordination.release_on_drop();
                coordination.wait_for_boundary(deadline).await?;
                require(observed.batch_effect_seen(&batch_artifact)?, "coordinated boundary has no actual completed batch effect")?;
                let (account_result, cards_result) = tokio::join!(
                    coordination.clone().guard_result(driver_spans.observe(16, campaign::resume_input(&app, &account))),
                    coordination.clone().guard_result(driver_spans.observe(17, campaign::resume_input(&app, &cards))),
                );
                Ok((account_result?, cards_result?))
            };
            // Both workloads finish and every submitted task joins before any
            // failure is interpreted or the original drain/cleanup can begin.
            let (joined, exchanges) = campaign::join_campaign(tasks, exchanges).await;
            let mut jobs = Vec::new(); let mut identities = BTreeSet::new();
            for result in joined {
                let (status, body) = result.map_err(|error| CorpusProblem::new("carddemo.full.compiled_overlap_drift", error.to_string()))??;
                require_terminal_status(status, StatusCode::CREATED, "compiled mixed job submission")?;
                let job: serde_json::Value = serde_json::from_slice(&body).map_err(json_problem)?;
                let id = job["jobid"].as_str().filter(|id| !id.is_empty()).ok_or_else(|| CorpusProblem::new("carddemo.full.compiled_overlap_drift", "missing job ID"))?.to_string();
                require(job["jobname"] == "READCUST" && job["owner"] == "IBMUSER" && identities.insert(id.clone()), "sixteen batch submissions must retain exact distinct owner/job identities")?;
                jobs.push(id);
            }
            retained["job_ids"] = serde_json::json!(jobs);
            let (account_result, cards_result) = exchanges?;
            let online_run = RunUnitId::new(account_result["run_unit"].as_str().ok_or_else(|| CorpusProblem::new("carddemo.full.compiled_overlap_drift", "online run identity missing"))?, InvocationLimits::default()).map_err(|_| CorpusProblem::new("carddemo.full.compiled_overlap_drift", "online run identity invalid"))?;
            require(account_result["session"] == account.session && cards_result["session"] == cards.session, "mixed session responses crossed identities")?;
            require(online_screen_fields(&account_result)? == online_screen_fields(&account_baseline)? && online_screen_fields(&cards_result)? == online_screen_fields(&cards_baseline)?, "mixed compiled screens differ from separate same-source outcomes")?;
            bms_seed_oracles::compare_account_view(&corpus_dir, &account_result, &online_screen_fields(&account_result)?, &mut RouteObservations::default())?;
            require(session_state(&app, &account).await? == account_result && session_state(&app, &cards).await? == cards_result, "stored mixed session state differs from each exact response")?;
            retained["account_response"] = account_result; retained["cards_response"] = cards_result;
            let mut completed = BTreeSet::new();
            loop {
                for id in &jobs {
                    let job = server.batch_service().get(id).map_err(terminal_problem)?;
                    if job.state.terminal() {
                        require(job.state == JobState::Completed && job.name == "READCUST" && job.owner == "IBMUSER" && job.return_code == Some(0) && job.steps.len() == 1 && job.steps[0].name == "STEP05" && job.steps[0].state == mainframe_env_batch::StepState::Completed && job.steps[0].termination == Some(mainframe_env_batch::StepTermination::ReturnCode { code: 0 }), "compiled mixed batch outcome/step differs")?;
                        completed.insert(id.clone());
                    }
                }
                if completed.len() == 16 { break; }
                require(tokio::time::Instant::now() < deadline, "sixteen compiled jobs did not complete inside original 120-second bound")?;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            require(tokio::time::Instant::now() <= deadline, "compiled mixed workload exceeded the unchanged 120-second completion bound")?;
            let mut outputs = serde_json::Map::new();
            for id in &jobs { outputs.insert(id.clone(), serde_json::json!(customer_spool(&server, id, &expected_spool)?)); }
            retained["raw_spool"] = serde_json::Value::Object(outputs);
            let after = shared_state(&server)?;
            retained["after"] = after.clone();
            require(before == after, "read-only compiled mixed workload changed shared raw records, identities, attributes or versions")?;
            let witness = observed.prove_overlap(&batch_artifact, &online_artifact, &online_run)?;
            coordination.finish()?;
            require(server.metrics().active == 0 && server.metrics().jes_active == 0, "compiled mixed workload retained active owners")?;
            retained["witness"] = serde_json::json!({"batch_execution":witness.batch_execution, "batch_run":witness.batch_run, "online_execution":witness.online_execution, "online_run":witness.online_run, "ordinals":witness.ordinals, "batch_effect_sequences":witness.batch_effect_sequences, "online_effect_sequence":witness.online_effect_sequence});
            Ok::<_, CorpusProblem>(())
        }.await;
        let drained = server.graceful_shutdown().await;
        retained["drained"] = serde_json::json!(drained);
        retained["problem"] = serde_json::json!(probe.as_ref().err().map(|problem| (&problem.code, &problem.detail)));
        let transcript = observed.export();
        retained["capture"] = transcript.as_ref().map_or_else(|problem| serde_json::json!({"problem":problem.detail}), Clone::clone);
        retained["driver_request_spans"] = driver_spans.export();
        retained["explicit_coordination"] = coordination.export();
        drop(app); drop(server);
        let saved = fs::write(evidence.join("actual.json"), serde_json::to_vec_pretty(&retained).map_err(json_problem)?).map_err(|error| CorpusProblem::new("carddemo.full.compiled_overlap_retention", error.to_string()));
        // Failed/unknown owners and retention failures preserve the fresh artifact namespace.
        if probe.is_ok() && drained && saved.is_ok() && transcript.is_ok() {
            let current = fs::symlink_metadata(&artifact_root).map_err(|error| CorpusProblem::new("carddemo.full.compiled_overlap_cleanup", error.to_string()))?;
            require(current.is_dir() && (current.dev(), current.ino()) == claimed_identity, "compiled mixed artifact namespace identity changed")?;
            fs::remove_dir_all(&artifact_root).map_err(|error| CorpusProblem::new("carddemo.full.compiled_overlap_cleanup", error.to_string()))?;
            require(!artifact_root.try_exists().map_err(|error| CorpusProblem::new("carddemo.full.compiled_overlap_cleanup", error.to_string()))?, "compiled mixed artifact namespace remains after cleanup")?;
            retained["cleanup_verified"] = serde_json::json!(true);
            fs::write(evidence.join("actual.json"), serde_json::to_vec_pretty(&retained).map_err(json_problem)?).map_err(|error| CorpusProblem::new("carddemo.full.compiled_overlap_retention", error.to_string()))?;
        }
        probe?; saved?; transcript?; require(drained, "compiled mixed server did not drain")
    });
    retained["producer_problem"] = serde_json::json!(
        result
            .as_ref()
            .err()
            .map(|problem| (&problem.code, &problem.detail))
    );
    fs::write(
        evidence.join("actual.json"),
        serde_json::to_vec_pretty(&retained).unwrap(),
    )
    .unwrap();
    result.unwrap();
    println!(
        "actual compiled overlap probe retains sixteen CBCUS01C jobs, separate COACTVWC/card sessions and default-control effect witness; zero journey/issue/floor/full-profile credit; evidence {}",
        evidence.display()
    );
}

#[test]
fn independent_full_spool_metadata_refuses_length_order_padding_and_digest_drift() {
    // Synthetic public literals, independently hashed by CPython in the external
    // delta generator; no customer records or application credit in this control.
    let rows = vec![b"FIRST  ".to_vec(), b"SECOND  ".to_vec()];
    let expected = vec![
        SpoolRecordExpected {
            ordinal: 0,
            bytes: 7,
            sha256: "49753f4a293c40839d2dad59c01dd1f66defa6c447683797e7db15bdfff20bb3".into(),
        },
        SpoolRecordExpected {
            ordinal: 1,
            bytes: 8,
            sha256: "f6482b65725b1787f03d8d536ce4d242bbc7d468073227265368b3e84b17ef76".into(),
        },
    ];
    compare_spool_records(&rows, false, &expected).unwrap();
    assert!(compare_spool_records(&rows[..1], false, &expected).is_err());
    let mut extra = rows.clone();
    extra.push(b"EXTRA".to_vec());
    assert!(compare_spool_records(&extra, false, &expected).is_err());
    assert!(compare_spool_records(&rows, true, &expected).is_err());
    let mut reordered = rows.clone();
    reordered.reverse();
    assert!(compare_spool_records(&reordered, false, &expected).is_err());
    let mut shortened = rows.clone();
    shortened[0].pop();
    assert!(compare_spool_records(&shortened, false, &expected).is_err());
    let mut padding = rows.clone();
    padding[0][6] = b'X';
    assert!(compare_spool_records(&padding, false, &expected).is_err());
    let mut ordinal = expected.clone();
    ordinal[1].ordinal = 2;
    assert!(compare_spool_records(&rows, false, &ordinal).is_err());
    let mut length = expected.clone();
    length[1].bytes += 1;
    assert!(compare_spool_records(&rows, false, &length).is_err());
    let mut digest = expected.clone();
    digest[1].sha256 = "0".repeat(64);
    assert!(compare_spool_records(&rows, false, &digest).is_err());
}
