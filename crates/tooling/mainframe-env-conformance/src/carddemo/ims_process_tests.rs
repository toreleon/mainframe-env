//! Process-exit proof: no retained host/runtime Arc can supply reopened data.
use super::*;

const CHILD: &str = "carddemo::ims_routes::tests::corpus_sqlite_process_child";
const ROOT_ENV: &str = "MAINFRAME_ENV_CARDEMO_IMS_PROCESS_ROOT";
const PHASE_ENV: &str = "MAINFRAME_ENV_CARDEMO_IMS_PROCESS_PHASE";

#[test]
#[ignore = "requires exact clean pinned CARDDEMO_CORPUS_DIR; run explicitly with --ignored"]
fn corpus_package_sqlite_recovers_after_process_exit() {
    let corpus = PathBuf::from(env::var_os(CORPUS_ENV).expect("CARDDEMO_CORPUS_DIR required"));
    verify_carddemo_corpus(
        &corpus,
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .unwrap();
    let root = env::temp_dir().join(format!(
        "carddemo-ims-process-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    for phase in ["seed", "reopen"] {
        let output = Command::new(env::current_exe().unwrap())
            .args(["--exact", CHILD, "--ignored", "--nocapture"])
            .env(ROOT_ENV, &root)
            .env(PHASE_ENV, phase)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{phase}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fs::remove_dir_all(root).unwrap();
}

// Only the explicitly invoked parent above enters this private subprocess test.
// Its acceptance result requires both child phases to execute and exit cleanly.
#[test]
#[ignore = "private subprocess entry; invoked by the process-exit regression"]
fn corpus_sqlite_process_child() {
    let root = PathBuf::from(env::var_os(ROOT_ENV).expect("parent-owned root required"));
    let corpus = PathBuf::from(env::var_os(CORPUS_ENV).expect("corpus required"));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    match env::var(PHASE_ENV).unwrap().as_str() {
        "seed" => {
            let observed = runtime
                .block_on(exercise_ims_routes_at(
                    &corpus,
                    StoreProfile::Sqlite,
                    &[],
                    Some(&root),
                ))
                .unwrap();
            assert_eq!(
                (
                    observed.roots,
                    observed.children,
                    observed.secondary_index_entries
                ),
                (2, 2, 2)
            );
        }
        "reopen" => runtime.block_on(async {
            let url = format!("sqlite://{}?mode=rwc", root.join("state.db").display());
            let config = ServerConfig {
                store_profile: StoreProfile::Sqlite,
                sqlite_url: url.clone(),
                artifact_root: root.clone(),
                tls: TlsConfig {
                    enabled: false,
                    certificate_path: None,
                    private_key_reference: None,
                },
                ..ServerConfig::default()
            };
            let store = open_store(StoreProfile::Sqlite, &url).unwrap();
            let server = ProductServer::open_with_package_trust(
                config,
                store.clone(),
                Arc::new(MemorySecretResolver::default()),
                default_program_router(),
                carddemo_package_trust().unwrap(),
            )
            .unwrap();
            let package = ims_packages::package(&corpus, 1, &[]).unwrap();
            let metadata = server
                .ims_service()
                .selected_metadata_generation(ims_packages::APPLICATION)
                .unwrap()
                .unwrap();
            assert_eq!(metadata.generation, 1);
            assert_eq!(
                metadata.package_identity,
                package_generation_identity(&package).unwrap()
            );
            assert_eq!(
                Some(&metadata.catalog),
                package.sections.ims_metadata.as_ref()
            );
            let ims = SelectedIms {
                server: &server,
                store,
            };
            let mut child = ims_record(200, b"99900001", b"CHILD-ONE").unwrap();
            child[8..12].copy_from_slice(b"EDIT");
            let expected = vec![
                ImsLoadRoot {
                    data: ims_record(100, b"000001", b"ROOT-ONE").unwrap(),
                    children: vec![child],
                },
                ImsLoadRoot {
                    data: ims_record(100, b"000002", b"ROOT-TWO").unwrap(),
                    children: vec![ims_record(200, b"99900002", b"CHILD-TWO").unwrap()],
                },
            ];
            assert_eq!(ims.hierarchy("DBPAUTP0").unwrap(), expected);
            assert_eq!(ims.secondary_index_entries("DBPAUTP0").unwrap(), 2);
            assert_eq!(server.ims_service().checkpoint_count().unwrap(), 1);
            verify_reopened_replay(&ims, &ims_invocation("ims-control", true, None).unwrap())
                .unwrap();
            assert!(server.graceful_shutdown().await);
        }),
        _ => panic!("unknown child phase"),
    }
}
