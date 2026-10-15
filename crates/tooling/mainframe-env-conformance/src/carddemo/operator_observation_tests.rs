//! Actual local installation and synthetic boundary controls, not full J19 credit.
use super::*;
use mainframe_env_store_api::{ProviderStateStore, ProviderStateWrite};

fn installed_fixture(control: impl FnOnce(&MemoryStore, &Path, &[BatchProgramDefinition])) {
    let artifact = crate::compile(crate::HELLO_SOURCE).unwrap();
    let definitions = (0..12)
        .map(|index| BatchProgramDefinition::current(format!("OP{index:06}"), &artifact))
        .collect::<Vec<_>>();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = env::temp_dir().canonicalize().unwrap().join(format!(
        "mainframe-env-j19-install-control-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let server = ProductServer::open(
            ServerConfig {
                store_profile: StoreProfile::Memory,
                artifact_root: root.clone(),
                tls: TlsConfig {
                    enabled: false,
                    certificate_path: None,
                    private_key_reference: None,
                },
                ..ServerConfig::default()
            },
            store.clone(),
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
        )
        .unwrap();
        let installed = server.install_batch_programs(definitions.clone()).unwrap();
        assert_eq!(installed.programs, 12);
        control(store.as_ref(), &root, &definitions);
        assert!(server.graceful_shutdown().await);
    });
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn actual_installed_rows_refuse_same_count_version_and_reference_drift() {
    installed_fixture(|store, root, definitions| {
        let expected = expected_programs(definitions).unwrap();
        assert!(compare_programs(&expected, store, root).is_ok());
        let mut row = store
            .get_provider_state("batch-program", "OP000000")
            .unwrap()
            .unwrap();
        let original_payload = row.payload.clone();
        row.version = 2;
        store
            .put_provider_states_atomic(vec![ProviderStateWrite {
                record: row.clone(),
                expected_version: Some(1),
            }])
            .unwrap();
        assert!(compare_programs(&expected, store, root).is_err());
        assert_eq!(expected.len(), 12);
        store
            .delete_provider_state("batch-program", "OP000000", 2)
            .unwrap();
        row.version = 1;
        row.payload[7] = if row.payload[7] == b'a' { b'b' } else { b'a' };
        assert_eq!(row.payload.len(), original_payload.len());
        store
            .put_provider_states_atomic(vec![ProviderStateWrite {
                record: row,
                expected_version: None,
            }])
            .unwrap();
        assert!(compare_programs(&expected, store, root).is_err());
    });
}

#[test]
fn actual_artifact_bytes_refuse_despite_unchanged_rows_and_reference() {
    installed_fixture(|store, root, definitions| {
        let expected = expected_programs(definitions).unwrap();
        assert!(compare_programs(&expected, store, root).is_ok());
        let digest = definitions[0]
            .artifact
            .as_str()
            .strip_prefix("sha256:")
            .unwrap();
        let object = root.join("objects").join(&digest[..2]).join(digest);
        let mut bytes = fs::read(&object).unwrap();
        let original_len = bytes.len();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(&object, &bytes).unwrap();
        assert_eq!(fs::metadata(&object).unwrap().len(), original_len as u64);
        for (name, (row, _)) in &expected {
            assert_eq!(
                store
                    .get_provider_state("batch-program", name)
                    .unwrap()
                    .as_ref(),
                Some(row)
            );
        }
        assert!(compare_programs(&expected, store, root).is_err());
    });
}

#[test]
#[cfg(unix)]
fn incomplete_restart_or_retained_artifacts_cannot_finish_owner() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = env::temp_dir().canonicalize().unwrap().join(format!(
        "mainframe-env-j19-cleanup-control-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    let owner = artifact_owner(&root).unwrap();
    assert!(require_finished(0, Some(&owner)).is_err());
    assert!(require_finished(1, Some(&owner)).is_err());
    assert!(require_finished(2, None).is_err());
    assert!(require_finished(2, Some(&owner)).is_err());
    fs::remove_dir_all(&root).unwrap();
    assert!(require_finished(1, Some(&owner)).is_err());
    // This tests only the actual cleanup guard, not a source- or runtime-bound proof.
    assert!(require_finished(2, Some(&owner)).is_ok());
}

#[test]
fn selected_download_refuses_method_path_status_and_same_length_byte_drift() {
    let path = "/zosmf/restfiles/ds/AWS.M2.CARDDEMO.FTP.TEST";
    assert!(compare_download(&Method::GET, path, StatusCode::OK, b"WELCOME-CD027").is_ok());
    for (method, selected_path, status, body) in [
        (Method::PUT, path, StatusCode::OK, &b"WELCOME-CD027"[..]),
        (
            Method::GET,
            "/zosmf/restfiles/ds/AWS.M2.CARDEMO.FTP.TEST",
            StatusCode::OK,
            &b"WELCOME-CD027"[..],
        ),
        (
            Method::GET,
            path,
            StatusCode::FORBIDDEN,
            &b"WELCOME-CD027"[..],
        ),
        (Method::GET, path, StatusCode::OK, &b"WELCOME-CD028"[..]),
    ] {
        assert!(compare_download(&method, selected_path, status, body).is_err());
    }
}

#[test]
fn selected_batch_definition_cannot_be_replaced_by_duplicate_names_or_counts() {
    let artifact = crate::compile(crate::HELLO_SOURCE).unwrap();
    let definitions = (0..12)
        .map(|index| BatchProgramDefinition::current(format!("OP{index:06}"), &artifact))
        .collect::<Vec<_>>();
    assert!(expected_programs(&definitions).is_ok());
    assert!(expected_programs(&definitions[..11]).is_err());
    let mut repeated = definitions.clone();
    repeated[11].name = repeated[0].name.clone();
    assert!(expected_programs(&repeated).is_err());
    let mut altered = definitions;
    altered[11].payload[0] ^= 1;
    assert!(expected_programs(&altered).is_err());
}
