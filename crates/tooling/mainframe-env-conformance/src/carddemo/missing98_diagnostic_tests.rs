//! A distinct missing-key98 diagnostic with no journey or issue observation.
//! Default key99, its exact comparator and its retained failure remain unchanged.
use super::*;
#[cfg(feature = "assurance-observers")]
#[path = "db2_copyback_owner_tests.rs"]
mod db2_copyback_owner_tests;
use mainframe_env_execution_api::RunUnitId;
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};

// Separately named batch-only owner reuses this private held receipt lease.
#[path = "db2_batch_source_owner.rs"]
mod batch_source_owner;

// Independently named real readonly IMS guest reuses only this held receipt lease.
#[path = "ims_firstparty_readonly_owner_tests.rs"]
mod ims_firstparty_readonly_owner_tests;

const KEY: &str = "98";
const SEED_KEYS: [&[u8]; 7] = [b"01", b"02", b"03", b"04", b"05", b"06", b"07"];
const EXTRA_SOURCE_PINS: &[(&str, &str)] = &[
    (
        "app/app-transaction-type-db2/jcl/CREADB21.jcl",
        "9ca64a0007f94cee65cdc2c4817ae433a72818831d654bf192613e6c6e7c4cc8",
    ),
    (
        "app/app-transaction-type-db2/ctl/DB2CREAT.ctl",
        "e6f3b60dc712f29f72f40168c8566ee713ef816a8ea90462daba5c9f71da106b",
    ),
    (
        "app/app-transaction-type-db2/ctl/DB2FREE.ctl",
        "042337a66816194c12594d9d940371f1ea413bab687455fd00a8216472bbdcaf",
    ),
    (
        "app/app-transaction-type-db2/ctl/DB2LTCAT.ctl",
        "1e0d56c17d52a71b36e7e52c261cb992394ae6f66be35e0f223b030ae5354602",
    ),
    (
        "app/app-transaction-type-db2/ctl/DB2LTTYP.ctl",
        "65d7c9b86b82d6b85bf50bfa18740c40353104148d1c3ce33dafc1a1a114fd6f",
    ),
    (
        "app/app-transaction-type-db2/ctl/DB2TEP41.ctl",
        "703636f7e6e5781f7c92f502e2923c839ae6d02a6be4cb8c1478433ca8f20f73",
    ),
    (
        "app/app-transaction-type-db2/ctl/DB2TIAD1.ctl",
        "4fcd0636295fded6a443b7908fc838fb6b6ec6fa6e803c9d2601ba64481e1101",
    ),
    (
        "app/app-transaction-type-db2/ctl/REPROCT.ctl",
        "5c6b8f26aa53c23131265e715b53130a1a7afd88b5bd3319aeca91f3d1a55bf5",
    ),
    (
        "app/cbl/COSGN00C.cbl",
        "4f901ae6b113eebae60ae1d94b30f8cb9906d031a4419770bf56980bdf97e377",
    ),
    (
        "app/bms/COSGN00.bms",
        "6747ba57e45fed7e52af728a1f8688ed8fab90b93e1234ab0a0bcae32fa39056",
    ),
    (
        "app/cpy-bms/COSGN00.CPY",
        "e2a2f8f6e76e1613cbd8f0baaca87d571a49a7bc219d16a732e4e4ecd7485407",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.USRSEC.PS",
        "8608d4b8a03f7587c1493282f9337ab24fdf527d0aadbdb85dc3713dccdc6fc8",
    ),
];

fn refused(detail: &str) -> CorpusProblem {
    CorpusProblem::new("carddemo.db2.missing98_diagnostic", detail)
}

fn require_absent98(rows: &[Vec<Vec<u8>>]) -> Result<(), CorpusProblem> {
    if rows.len() != SEED_KEYS.len()
        || rows.iter().zip(SEED_KEYS).any(|(row, expected)| {
            row.len() != 2 || row[0].as_slice() != expected || row[0].as_slice() == KEY.as_bytes()
        })
    {
        return Err(refused(
            "key98 absence requires all seven independently pinned ordered seed keys",
        ));
    }
    Ok(())
}

fn metadata(actual: Option<&[u8]>, expected: &[u8]) -> serde_json::Value {
    let first_difference = actual.and_then(|actual| {
        (actual != expected).then(|| {
            actual
                .iter()
                .zip(expected)
                .position(|(actual, expected)| actual != expected)
                .unwrap_or(actual.len().min(expected.len()))
        })
    });
    serde_json::json!({
        "present":actual.is_some(), "actual_bytes":actual.map(<[u8]>::len),
        "actual_sha256":actual.map(|bytes| format!("{:x}", Sha256::digest(bytes))),
        "expected_bytes":expected.len(), "expected_sha256":format!("{:x}", Sha256::digest(expected)),
        "first_difference":first_difference,
    })
}

// Exact bytes are authorized only for these five fixed technical/error fields
// in the private externally retained 0600 receipt. Never copy the response or
// iterate its fields into an exact-byte projection; other values remain absent.
fn private_five_field_bytes(
    response: &serde_json::Value,
    fields: Option<&BTreeMap<String, Vec<u8>>>,
) -> serde_json::Value {
    let encode = |bytes: Option<&[u8]>| {
        bytes.map(|bytes| base64::engine::general_purpose::STANDARD.encode(bytes))
    };
    serde_json::json!({
        "mapset":encode(response["mapset"].as_str().map(str::as_bytes)),
        "map":encode(response["map"].as_str().map(str::as_bytes)),
        "PGMNAME":encode(fields.and_then(|fields| fields.get("PGMNAME")).map(Vec::as_slice)),
        "TRNNAME":encode(fields.and_then(|fields| fields.get("TRNNAME")).map(Vec::as_slice)),
        "ERRMSG":encode(fields.and_then(|fields| fields.get("ERRMSG")).map(Vec::as_slice)),
    })
}

fn screen_failure_context(response: &serde_json::Value) -> serde_json::Value {
    // This is diagnostic metadata, never a substitute error predicate. The
    // actual predicate is the unchanged db2_online_rows::require_missing.
    // The actual product DTO exports run_unit. Shape validation is diagnostic
    // attribution only; it does not mint actor or execution authority.
    let dto_schema_matches = response["schema_version"] == "mainframe-env.cics-terminal@1";
    let run = dto_schema_matches
        .then(|| response["run_unit"].as_str())
        .flatten()
        .and_then(|value| RunUnitId::new(value, InvocationLimits::default()).ok());
    let run_digest = run
        .as_ref()
        .map(|run| format!("{:x}", Sha256::digest(run.as_str().as_bytes())));
    let fields = match online_screen_fields(response) {
        Ok(fields) => fields,
        Err(problem) => {
            return serde_json::json!({"decode_problem_code":problem.code,
            "dto_schema_matches":dto_schema_matches,
            "run_unit_valid":run.is_some(), "run_unit_sha256":run_digest,
            "mapset":metadata(response["mapset"].as_str().map(str::as_bytes), b"COTRTUP"),
            "map":metadata(response["map"].as_str().map(str::as_bytes), b"CTRTUPA"),
            "exact_field_bytes_base64":private_five_field_bytes(response, None)});
        }
    };
    let mut names = Sha256::new();
    for name in fields.keys() {
        digest_field(&mut names, name.as_bytes());
    }
    serde_json::json!({
        "dto_schema_matches":dto_schema_matches,
            "run_unit_valid":run.is_some(), "run_unit_sha256":run_digest,
        "session_sha256":response["session"].as_str().map(|value| format!("{:x}", Sha256::digest(value.as_bytes()))),
        "field_count":fields.len(), "ordered_field_names_sha256":format!("{:x}", names.finalize()),
        "exact_field_bytes_base64":private_five_field_bytes(response, Some(&fields)),
        "mapset":metadata(response["mapset"].as_str().map(str::as_bytes), b"COTRTUP"),
        "map":metadata(response["map"].as_str().map(str::as_bytes), b"CTRTUPA"),
        "PGMNAME":metadata(fields.get("PGMNAME").map(Vec::as_slice), b"COTRTUPC"),
        "TRNNAME":metadata(fields.get("TRNNAME").map(Vec::as_slice), b"CTTU"),
        "ERRMSG":metadata(fields.get("ERRMSG").map(Vec::as_slice),
            b"No record found for this key in database                                      "),
    })
}

fn require_initial_menu_context(menu: &CardDemoOnlineSession) -> Result<(), CorpusProblem> {
    if menu.menu_response["schema_version"] != "mainframe-env.cics-terminal@1"
        || menu.menu_response["session"].as_str() != Some(menu.session.as_str())
        || menu.menu_response["principal"] != "WEBADM"
    {
        return Err(refused(
            "fresh admin menu DTO schema/session/principal differs",
        ));
    }
    Ok(())
}

fn rows_failure_context(server: &ProductServer) -> serde_json::Value {
    let mut result = serde_json::Map::new();
    for (table, limit) in [
        ("CARDDEMO.TRANSACTION_TYPE", 8),
        ("CARDDEMO.TRANSACTION_TYPE_CATEGORY", 19),
    ] {
        let observation = match server.db2_service().table_rows(table) {
            Ok(rows) => {
                let bounded = rows
                    .iter()
                    .take(limit)
                    .enumerate()
                    .map(|(ordinal, row)| {
                        let mut digest = Sha256::new();
                        digest.update((row.len() as u64).to_be_bytes());
                        for column in row {
                            digest_field(&mut digest, column);
                        }
                        serde_json::json!({"ordinal":ordinal, "columns":row.len(),
                        "column_bytes":row.iter().map(Vec::len).collect::<Vec<_>>(),
                        "framed_row_sha256":format!("{:x}", digest.finalize())})
                    })
                    .collect::<Vec<_>>();
                serde_json::json!({"row_count":rows.len(), "retained_rows":bounded,
                    "bounded_diagnostic_truncation":rows.len() > limit})
            }
            Err(_) => serde_json::json!({"read_failed":true}),
        };
        result.insert(table.into(), observation);
    }
    serde_json::Value::Object(result)
}

fn require_owned_directory(path: &Path, expected: (u64, u64)) -> Result<(), CorpusProblem> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| refused("owned directory identity unavailable"))?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || (metadata.dev(), metadata.ino()) != expected
        || metadata.permissions().mode() & 0o777 != 0o700
        || path
            .canonicalize()
            .map_err(|_| refused("owned directory canonical identity unavailable"))?
            != path
    {
        return Err(refused(
            "owned directory identity changed; preserve artifacts",
        ));
    }
    Ok(())
}

// Keep the originally admitted file descriptor. Pre/post path observations
// refuse replacements; writes never reopen a rebound receipt path. These fixed
// private-owner observations are not atomic filesystem/process containment.
struct ReceiptOwner {
    evidence: PathBuf,
    evidence_identity: (u64, u64),
    artifacts: PathBuf,
    artifact_identity: (u64, u64),
    file: fs::File,
    file_identity: (u64, u64),
    artifacts_removed: bool,
    retention_failed: bool,
}

impl ReceiptOwner {
    fn open(
        evidence: &Path,
        evidence_identity: (u64, u64),
        artifacts: &Path,
        artifact_identity: (u64, u64),
    ) -> Result<Self, CorpusProblem> {
        require_owned_directory(evidence, evidence_identity)?;
        require_owned_directory(artifacts, artifact_identity)?;
        // create_new refuses existing files/symlinks. No receipt bytes are
        // written until both roots and this exact private handle are rechecked.
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(evidence.join("actual.json"))
            .map_err(|_| refused("exclusive receipt admission failed"))?;
        let metadata = file
            .metadata()
            .map_err(|_| refused("receipt handle identity unavailable"))?;
        let owner = Self {
            evidence: evidence.into(),
            evidence_identity,
            artifacts: artifacts.into(),
            artifact_identity,
            file,
            file_identity: (metadata.dev(), metadata.ino()),
            artifacts_removed: false,
            retention_failed: false,
        };
        owner.check()?;
        Ok(owner)
    }

    fn check(&self) -> Result<(), CorpusProblem> {
        if self.retention_failed {
            return Err(refused("retention already refused; preserve artifacts"));
        }
        require_owned_directory(&self.evidence, self.evidence_identity)?;
        if self.artifacts_removed {
            match fs::symlink_metadata(&self.artifacts) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(refused("checked artifact absence changed or unknown")),
            }
        } else {
            require_owned_directory(&self.artifacts, self.artifact_identity)?;
        }
        let path = fs::symlink_metadata(self.evidence.join("actual.json"))
            .map_err(|_| refused("receipt path identity unavailable"))?;
        let handle = self
            .file
            .metadata()
            .map_err(|_| refused("receipt handle identity unavailable"))?;
        if [&path, &handle].into_iter().any(|metadata| {
            !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.nlink() != 1
                || metadata.permissions().mode() & 0o777 != 0o600
                || (metadata.dev(), metadata.ino()) != self.file_identity
        }) {
            return Err(refused(
                "receipt path or handle identity changed; preserve artifacts",
            ));
        }
        Ok(())
    }

    fn save(&mut self, receipt: &serde_json::Value) -> Result<(), CorpusProblem> {
        let saved = (|| {
            self.check()?;
            let bytes = serde_json::to_vec_pretty(receipt)
                .map_err(|_| refused("receipt encoding failed"))?;
            self.file
                .seek(SeekFrom::Start(0))
                .map_err(|_| refused("receipt seek failed"))?;
            self.file
                .write_all(&bytes)
                .map_err(|_| refused("receipt retention failed"))?;
            self.file
                .set_len(bytes.len() as u64)
                .map_err(|_| refused("receipt length retention failed"))?;
            self.file
                .sync_all()
                .map_err(|_| refused("receipt sync failed"))?;
            self.check()
        })();
        // A failed identity/write/retention stage cannot later authorize server
        // admission or artifact cleanup; retain the roots and any actual bytes.
        if saved.is_err() {
            self.retention_failed = true;
        }
        saved
    }
}

#[test]
#[ignore = "new input98 diagnostic; exact clean CARDDEMO_CORPUS_DIR and fresh external CARDDEMO_DB2_MISSING98_EVIDENCE required; zero acceptance credit"]
fn pinned_corpus_new_missing98_preserves_exact_error_oracle() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let corpus = verify_carddemo_corpus_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .unwrap();
    let corpus_dir = PathBuf::from(env::var_os(CORPUS_ENV).unwrap());
    db2_online_rows::check_sources(&corpus_dir).unwrap();
    for (path, expected) in EXTRA_SOURCE_PINS {
        let bytes = read_corpus_file(&corpus_dir, &corpus_dir.join(path)).unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(&bytes)), *expected);
    }
    let offered = PathBuf::from(
        env::var_os("CARDDEMO_DB2_MISSING98_EVIDENCE")
            .expect("fresh external evidence path required"),
    );
    let parent = offered.parent().unwrap().canonicalize().unwrap();
    assert!(
        !parent.starts_with(root.canonicalize().unwrap()),
        "diagnostic evidence must remain outside Git"
    );
    let evidence = parent.join(offered.file_name().unwrap());
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&evidence)
        .expect("evidence path must be fresh and exclusively owned");
    let evidence_claim = fs::symlink_metadata(&evidence).unwrap();
    let evidence_identity = (evidence_claim.dev(), evidence_claim.ino());
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
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut receipt = serde_json::json!({
        "schema":"external.carddemo-missing98-diagnostic@1", "acceptance_credit":0,
        "corpus_commit":corpus.commit, "input_key":KEY, "transport_actor":"WEBADM",
        "application_user":"ADMIN001", "transaction":"CTTU", "program":"COTRTUPC",
        "store":"memory", "whole_state_retained":false, "stage":"before-open",
        "source_pins":EXTRA_SOURCE_PINS, "artifact_root":artifacts, "artifact_identity":artifact_identity,
        "controls":"default configuration, clock, router, 30-second HTTP and original job completion bounds",
        "original99_owner_unchanged":true, "drained":false, "cleanup_verified":false,
    });
    owned.save(&receipt).unwrap();
    let result = runtime.block_on(async {
        owned.check()?;
        let server = ProductServer::open_with_package_trust(
            ServerConfig {
                store_profile: StoreProfile::Memory,
                artifact_root: artifacts.clone(),
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
            carddemo_package_trust()?,
        )
        .map_err(terminal_problem)?;
        let app = server.router();
        let mut response = None;
        let probe = async {
            // Check after artifact-store opening, before any installed offers.
            owned.check()?;
            receipt["stage"] = serde_json::json!("install");
            let (online, _) = carddemo_db2_online_definition(&corpus_dir)?;
            receipt["compiled_selected_artifacts"] = serde_json::json!(
                online
                    .programs
                    .iter()
                    .filter(|definition| ["COSGN00C", "COADM01C", "COTRTUPC"]
                        .contains(&definition.name.as_str()))
                    .map(|definition| (&definition.name, definition.artifact.as_str()))
                    .collect::<Vec<_>>()
            );
            // Existing local selected catalog/controller setup; none of its
            // handwritten controllers receives first-party application credit.
            install_carddemo_db2_package(&server, &[])?;
            server
                .bootstrap_user("IBMUSER", b"TESTPASS")
                .map_err(terminal_problem)?;
            install_base_online_authorities(&server, &corpus_dir, &online)?;
            online_authorities::install_db2_authorities(&server)?;
            owned.check()?;
            server
                .install_online_application(online)
                .map_err(terminal_problem)?;
            owned.check()?;
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
            for dataset in [
                "AWS.M2.CARDDEMO.TRANTYPE.VSAM.KSDS",
                "AWS.M2.CARDDEMO.TRANCATG.VSAM.KSDS",
            ] {
                racf.permit("DATASET", dataset, "IBMUSER", AccessIntent::Alter)
                    .map_err(terminal_problem)?;
            }
            let mut members = Vec::new();
            for relative in
                collect_paths(&corpus_dir, &["app/app-transaction-type-db2/ctl"], "ctl")?
            {
                let name = Path::new(&relative)
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| refused("invalid control name"))?;
                let bytes = read_corpus_file(&corpus_dir, &corpus_dir.join(&relative))?;
                let text = String::from_utf8(bytes).map_err(|_| refused("control is not UTF-8"))?;
                members.push((
                    name.into(),
                    text.lines().map(|line| line.as_bytes().to_vec()).collect(),
                ));
            }
            control_library::seed_control_library(
                &server,
                "AWS.M2.CARDDEMO.CNTL",
                members,
                &mut 40_000,
            )?;
            receipt["stage"] = serde_json::json!("first-party-seed-job");
            let jcl = String::from_utf8(read_corpus_file(
                &corpus_dir,
                &corpus_dir.join("app/app-transaction-type-db2/jcl/CREADB21.jcl"),
            )?)
            .map_err(|_| refused("JCL is not UTF-8"))?;
            let job = submit_job_with_retcode(&server, &app, &jcl, "CC 0000").await?;
            receipt["seed_job"] = serde_json::json!(job);
            db2_online_rows::require_rows(&server, db2_online_rows::Phase::Seed)?;
            require_absent98(
                &server
                    .db2_service()
                    .table_rows("CARDDEMO.TRANSACTION_TYPE")
                    .map_err(terminal_problem)?,
            )?;
            receipt["stage"] = serde_json::json!("fresh-admin-menu");
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
            let selected = carddemo_terminal_exchange(
                &app,
                &menu.session,
                &menu.headers,
                0x7d,
                BTreeMap::from([("OPTION".into(), "6".into())]),
            )
            .await?;
            require_online_mapset(&selected, "COTRTUP", "new98 CTTU launch")?;
            receipt["stage"] = serde_json::json!("new-missing98-offer");
            let missing = carddemo_terminal_exchange(
                &app,
                &menu.session,
                &menu.headers,
                0x7d,
                BTreeMap::from([("TRTYPCD".into(), KEY.into())]),
            )
            .await?;
            response = Some(missing);
            let missing = response
                .as_ref()
                .ok_or_else(|| refused("missing response unavailable"))?;
            let screen = db2_online_rows::require_missing(missing);
            let rows = db2_online_rows::require_rows(&server, db2_online_rows::Phase::Seed);
            screen?;
            rows?;
            receipt["stage"] = serde_json::json!("new98-exact-predicate-matched");
            Ok::<_, CorpusProblem>(())
        }
        .await;
        // Capture while this Memory owner is still alive, only on actual refusal.
        // These bounded digests do not preserve or certify complete live state.
        if let Err(problem) = &probe {
            receipt["problem_code"] = serde_json::json!(problem.code);
            receipt["failure_rows"] = rows_failure_context(&server);
            if let Some(response) = &response {
                receipt["failure_screen"] = screen_failure_context(response);
            }
        }
        let before_drain_saved = owned.save(&receipt);
        drop(app);
        let drained = server.graceful_shutdown().await;
        receipt["drained"] = serde_json::json!(drained);
        drop(server);
        let retained = owned.save(&receipt);
        if probe.is_ok() && drained && before_drain_saved.is_ok() && retained.is_ok() {
            owned.check()?;
            let current_evidence = fs::symlink_metadata(&evidence)
                .map_err(|_| refused("evidence identity unavailable"))?;
            let current = fs::symlink_metadata(&artifacts)
                .map_err(|_| refused("artifact identity unavailable"))?;
            if !current_evidence.is_dir()
                || (current_evidence.dev(), current_evidence.ino()) != evidence_identity
                || !current.is_dir()
                || (current.dev(), current.ino()) != artifact_identity
            {
                return Err(refused(
                    "exclusive owner identity changed; preserve artifacts",
                ));
            }
            fs::remove_dir_all(&artifacts)
                .map_err(|_| refused("checked artifact cleanup failed"))?;
            match fs::symlink_metadata(&artifacts) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Ok(_) => return Err(refused("artifact root remains after cleanup")),
                Err(_) => return Err(refused("cleanup absence unknown")),
            }
            owned.artifacts_removed = true;
            owned.check()?;
            receipt["cleanup_verified"] = serde_json::json!(true);
            owned.save(&receipt)?;
        }
        probe?;
        before_drain_saved?;
        retained?;
        if !drained {
            return Err(refused("checked drain refused; preserve artifacts"));
        }
        Ok(())
    });
    if let Err(problem) = &result {
        receipt["producer_problem_code"] = serde_json::json!(problem.code);
    }
    owned.save(&receipt).unwrap();
    // Do not print helper error details, which may contain an HTTP body/trace.
    assert!(
        result.is_ok(),
        "new98 diagnostic refused; zero credit; bounded external context retained at {}",
        evidence.display()
    );
    assert_eq!(receipt["cleanup_verified"], true);
}

#[test]
fn independent_seven_seed_keys_refuse_missing98_seed_drift() {
    let rows = SEED_KEYS
        .iter()
        .map(|key| vec![key.to_vec(), b"PUBLIC DESCRIPTION".to_vec()])
        .collect::<Vec<_>>();
    require_absent98(&rows).unwrap();
    let mut present = rows.clone();
    present[6][0] = b"98".to_vec();
    assert!(require_absent98(&present).is_err());
    let mut duplicate = rows.clone();
    duplicate[6][0] = b"06".to_vec();
    assert!(require_absent98(&duplicate).is_err());
    assert!(require_absent98(&rows[..6]).is_err());
    let mut extra = rows.clone();
    extra.push(vec![b"99".to_vec(), Vec::new()]);
    assert!(require_absent98(&extra).is_err());
}

#[test]
fn failure_context_retains_padding_offsets_without_raw_values() {
    let canary = b"PRIVATE-CANARY";
    let result = metadata(Some(canary), b"PUBLIC EXPECTATION");
    let encoded = serde_json::to_string(&result).unwrap();
    assert!(!encoded.contains("PRIVATE-CANARY") && !encoded.contains("PUBLIC EXPECTATION"));
    assert_eq!(result["first_difference"], 1);
    let padding = metadata(Some(b"ABC X"), b"ABC  ");
    assert_eq!(padding["first_difference"], 4);
    let shortened = metadata(Some(b"ABC"), b"ABC  ");
    assert_eq!(shortened["first_difference"], 3);
    assert_eq!(metadata(None, b"ABC")["present"], false);
    assert!(metadata(Some(b"ABC"), b"ABC")["first_difference"].is_null());
}

#[test]
fn actual_dto_run_unit_is_validated_hashed_and_not_replaced_by_run_alias() {
    let response = serde_json::json!({"schema_version":"mainframe-env.cics-terminal@1",
        "run_unit":"actual-run-98", "run":"foreign-decoy",
        "screen_base64":"", "session":"private-session"});
    let context = screen_failure_context(&response);
    assert_eq!(context["run_unit_valid"], true);
    assert_eq!(
        context["run_unit_sha256"],
        format!("{:x}", Sha256::digest(b"actual-run-98"))
    );
    let encoded = serde_json::to_string(&context).unwrap();
    assert!(
        !encoded.contains("actual-run-98")
            && !encoded.contains("foreign-decoy")
            && !encoded.contains("private-session")
    );
    for value in [
        serde_json::Value::Null,
        serde_json::json!(""),
        serde_json::json!("bad run"),
    ] {
        let mut invalid = response.clone();
        invalid["run_unit"] = value;
        let context = screen_failure_context(&invalid);
        assert_eq!(context["run_unit_valid"], false);
        assert!(context["run_unit_sha256"].is_null());
    }
    for schema in [
        serde_json::Value::Null,
        serde_json::json!("foreign-terminal@1"),
    ] {
        let mut invalid = response.clone();
        invalid["schema_version"] = schema;
        let context = screen_failure_context(&invalid);
        assert_eq!(context["dto_schema_matches"], false);
        assert_eq!(context["run_unit_valid"], false);
        assert!(context["run_unit_sha256"].is_null());
    }
    let mut malformed_screen = response;
    malformed_screen["screen_base64"] = serde_json::json!("!");
    let context = screen_failure_context(&malformed_screen);
    assert_eq!(context["run_unit_valid"], true);
    assert_eq!(
        context["run_unit_sha256"],
        format!("{:x}", Sha256::digest(b"actual-run-98"))
    );
    assert!(!context["decode_problem_code"].is_null());
}

#[test]
fn replaced_evidence_artifact_or_receipt_refuses_writes_and_preserves_owned_bytes() {
    let parent = env::temp_dir().canonicalize().unwrap();
    for replacement in ["evidence", "artifact", "receipt"] {
        let mut nonce_digest = Sha256::new();
        digest_field(&mut nonce_digest, &std::process::id().to_be_bytes());
        let nonce_tick = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        digest_field(&mut nonce_digest, &nonce_tick.to_be_bytes());
        digest_field(&mut nonce_digest, replacement.as_bytes());
        let nonce = format!("{:x}", nonce_digest.finalize());
        let evidence = parent.join(format!("carddemo-missing98-controls-{nonce}"));
        fs::DirBuilder::new().mode(0o700).create(&evidence).unwrap();
        let artifacts = evidence.join("owned-artifacts");
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&artifacts)
            .unwrap();
        let identity = |path: &Path| {
            let metadata = fs::symlink_metadata(path).unwrap();
            (metadata.dev(), metadata.ino())
        };
        let evidence_identity = identity(&evidence);
        let artifact_identity = identity(&artifacts);
        let mut owned =
            ReceiptOwner::open(&evidence, evidence_identity, &artifacts, artifact_identity)
                .unwrap();
        owned
            .save(&serde_json::json!({"stage":"owned", "credit":0}))
            .unwrap();
        let before = fs::read(evidence.join("actual.json")).unwrap();
        let replaced = match replacement {
            "evidence" => evidence.clone(),
            "artifact" => artifacts.clone(),
            _ => evidence.join("actual.json"),
        };
        let retained = replaced.with_file_name(format!(
            "{}-retained",
            replaced.file_name().unwrap().to_str().unwrap()
        ));
        fs::rename(&replaced, &retained).unwrap();
        if replacement == "receipt" {
            let mut foreign = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&replaced)
                .unwrap();
            foreign.write_all(b"FOREIGN-CANARY").unwrap();
        } else {
            fs::DirBuilder::new().mode(0o700).create(&replaced).unwrap();
            fs::write(replaced.join("foreign-canary"), b"FOREIGN-CANARY").unwrap();
        }
        let replacement_identity = identity(&replaced);
        assert!(
            owned
                .save(&serde_json::json!({"stage":"must-not-write"}))
                .is_err()
        );
        assert!(owned.retention_failed && owned.check().is_err());
        let canary = if replacement == "receipt" {
            replaced.clone()
        } else {
            replaced.join("foreign-canary")
        };
        assert_eq!(fs::read(&canary).unwrap(), b"FOREIGN-CANARY");
        let original_receipt = match replacement {
            "evidence" => retained.join("actual.json"),
            "receipt" => retained.clone(),
            _ => evidence.join("actual.json"),
        };
        assert_eq!(fs::read(&original_receipt).unwrap(), before);
        assert!(retained.exists() && replaced.exists());
        // Only exact control-created replacement entries are removed. Restore
        // the original namespace; the refused owner is never readmitted.
        assert_eq!(identity(&replaced), replacement_identity);
        if replacement == "receipt" {
            fs::remove_file(&replaced).unwrap();
        } else {
            fs::remove_file(canary).unwrap();
            fs::remove_dir(&replaced).unwrap();
        }
        fs::rename(&retained, &replaced).unwrap();
        assert!(owned.check().is_err());
        drop(owned);
        require_owned_directory(&evidence, evidence_identity).unwrap();
        require_owned_directory(&artifacts, artifact_identity).unwrap();
        assert_eq!(fs::read(evidence.join("actual.json")).unwrap(), before);
        fs::remove_dir_all(&evidence).unwrap();
        assert!(matches!(fs::symlink_metadata(&evidence), Err(error)
            if error.kind() == std::io::ErrorKind::NotFound));
    }
}

#[test]
fn initial_menu_dto_refuses_wrong_schema_session_and_principal() {
    let response = serde_json::json!({"schema_version":"mainframe-env.cics-terminal@1",
        "session":"actual-menu-session", "principal":"WEBADM"});
    let menu = |response| CardDemoOnlineSession {
        session: "actual-menu-session".into(),
        headers: BTreeMap::new(),
        menu_response: response,
    };
    require_initial_menu_context(&menu(response.clone())).unwrap();
    for (field, value) in [
        ("schema_version", "foreign-terminal@1"),
        ("session", "foreign-session"),
        ("principal", "WEBREG"),
    ] {
        let mut wrong = response.clone();
        wrong[field] = serde_json::json!(value);
        assert!(require_initial_menu_context(&menu(wrong)).is_err());
    }
    let mut absent = response;
    absent.as_object_mut().unwrap().remove("schema_version");
    assert!(require_initial_menu_context(&menu(absent)).is_err());
}

fn diagnostic_response_for_controls(fields: BTreeMap<String, Vec<u8>>) -> serde_json::Value {
    let mut bytes = Vec::new();
    for (name, value) in fields {
        bytes.extend_from_slice(&(name.len() as u32).to_be_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&value);
    }
    serde_json::json!({"schema_version":"mainframe-env.cics-terminal@1",
        "run_unit":"synthetic-run-98", "mapset":"COTRTUP", "map":"CTRTUPA",
        "screen_base64":base64::engine::general_purpose::STANDARD.encode(bytes),
        "session":"EXCLUDED-SESSION-CANARY", "bearer":"EXCLUDED-BEARER-CANARY",
        "customer":"EXCLUDED-TOP-CUSTOMER-CANARY"})
}

#[test]
fn private_exact_projection_has_only_five_fields_and_hashes_back_to_metadata() {
    let expected = BTreeMap::from([
        ("mapset", b"COTRTUP".to_vec()),
        ("map", b"CTRTUPA".to_vec()),
        ("PGMNAME", b"COTRTUPC".to_vec()),
        ("TRNNAME", b"CTTU".to_vec()),
        ("ERRMSG", vec![0, 0xff, b'A', b' ', b' ']),
    ]);
    let fields = BTreeMap::from([
        ("PGMNAME".into(), expected["PGMNAME"].clone()),
        ("TRNNAME".into(), expected["TRNNAME"].clone()),
        ("ERRMSG".into(), expected["ERRMSG"].clone()),
        (
            "UNEXPECTED-PRIVATE-FIELD".into(),
            b"EXCLUDED-OTHER-FIELD-CANARY".to_vec(),
        ),
        ("PASSWD".into(), b"EXCLUDED-PASSWORD-CANARY".to_vec()),
        (
            "CUSTNAME".into(),
            b"EXCLUDED-CUSTOMER-FIELD-CANARY".to_vec(),
        ),
    ]);
    let response = diagnostic_response_for_controls(fields);
    let context = screen_failure_context(&response);
    let exact = context["exact_field_bytes_base64"].as_object().unwrap();
    assert!(
        exact.len() == 5
            && exact
                .keys()
                .all(|name| expected.contains_key(name.as_str())),
        "exact-byte projection must retain only the five fixed names"
    );
    for (name, expected) in expected {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(exact[name].as_str().unwrap())
            .unwrap();
        assert!(decoded == expected, "allowlisted exact bytes differ");
        assert!(
            context[name]["actual_bytes"].as_u64() == Some(decoded.len() as u64),
            "exact bytes must match retained byte length"
        );
        let digest = format!("{:x}", Sha256::digest(&decoded));
        assert!(
            context[name]["actual_sha256"].as_str() == Some(digest.as_str()),
            "exact bytes must hash to retained metadata"
        );
    }
    let encoded = serde_json::to_string(&context).unwrap();
    for excluded in [
        "EXCLUDED-SESSION-CANARY",
        "EXCLUDED-BEARER-CANARY",
        "EXCLUDED-TOP-CUSTOMER-CANARY",
        "EXCLUDED-OTHER-FIELD-CANARY",
        "EXCLUDED-PASSWORD-CANARY",
        "EXCLUDED-CUSTOMER-FIELD-CANARY",
    ] {
        let base64 = base64::engine::general_purpose::STANDARD.encode(excluded.as_bytes());
        assert!(
            !encoded.contains(excluded) && !encoded.contains(&base64),
            "unexpected/private values must not enter the exact projection"
        );
    }
    assert!(!encoded.contains("screen_base64") && !encoded.contains("UNEXPECTED-PRIVATE-FIELD"));
}

#[test]
fn private_exact_projection_distinguishes_unavailable_empty_and_decode_failure() {
    let response = diagnostic_response_for_controls(BTreeMap::new());
    let context = screen_failure_context(&response);
    for name in ["PGMNAME", "TRNNAME", "ERRMSG"] {
        assert!(context["exact_field_bytes_base64"][name].is_null());
        assert!(context[name]["present"] == false);
    }
    let response = diagnostic_response_for_controls(BTreeMap::from([
        ("PGMNAME".into(), Vec::new()),
        ("TRNNAME".into(), Vec::new()),
        ("ERRMSG".into(), Vec::new()),
    ]));
    let context = screen_failure_context(&response);
    for name in ["PGMNAME", "TRNNAME", "ERRMSG"] {
        assert!(context["exact_field_bytes_base64"][name] == "");
        assert!(context[name]["present"] == true && context[name]["actual_bytes"] == 0);
        let digest = format!("{:x}", Sha256::digest(b""));
        assert!(context[name]["actual_sha256"].as_str() == Some(digest.as_str()));
    }
    let mut unavailable = response;
    unavailable["mapset"] = serde_json::Value::Null;
    unavailable["map"] = serde_json::Value::Null;
    let context = screen_failure_context(&unavailable);
    for name in ["mapset", "map"] {
        assert!(context["exact_field_bytes_base64"][name].is_null());
    }
    let mut malformed = diagnostic_response_for_controls(BTreeMap::new());
    malformed["screen_base64"] = serde_json::json!("!");
    let context = screen_failure_context(&malformed);
    assert!(!context["decode_problem_code"].is_null());
    let exact = context["exact_field_bytes_base64"].as_object().unwrap();
    assert!(exact.len() == 5);
    for (name, expected) in [
        ("mapset", b"COTRTUP".as_slice()),
        ("map", b"CTRTUPA".as_slice()),
    ] {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(exact[name].as_str().unwrap())
            .unwrap();
        assert!(
            decoded == expected,
            "known DTO map bytes must survive malformed screen decoding"
        );
        let digest = format!("{:x}", Sha256::digest(&decoded));
        assert!(context[name]["actual_sha256"].as_str() == Some(digest.as_str()));
    }
    // An undecodable screen supplies no fabricated field values.
    for name in ["PGMNAME", "TRNNAME", "ERRMSG"] {
        assert!(exact[name].is_null());
    }
}
