//! Test-only complete-profile setup from the existing owner and import planner.
//! No native execution, startup normalization, source or SAF credit.
use super::*;

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SetupRow {
    namespace: String,
    key: String,
    version: u64,
    payload: Vec<u8>,
}

fn emit(version: i32, variable: &str) {
    let mut stages = Vec::new();
    let fixture = NativeFixture::with_setup_capture(false, version, false, None, |store| {
        let rows = store.list_provider_state_prefix("mq-", 4096).unwrap();
        assert!(rows.len() < 4096);
        stages.push(
            rows.into_iter()
                .map(|row| SetupRow {
                    namespace: row.namespace,
                    key: row.key,
                    version: row.version,
                    payload: row.payload,
                })
                .collect::<Vec<_>>(),
        );
    });
    assert_eq!(stages.len(), 4);
    assert!(stages[1..].iter().all(|rows| !rows.is_empty()));
    assert!(
        fixture
            .rows()
            .iter()
            .all(|row| row.namespace.starts_with("mq-"))
    );
    let mut bytes = serde_json::to_vec_pretty(&stages).unwrap();
    bytes.push(b'\n');
    assert_eq!(
        serde_json::from_slice::<Vec<Vec<SetupRow>>>(&bytes).unwrap(),
        stages
    );
    assert!(bytes.len() < 1 << 20);
    if let Some(path) = std::env::var_os(variable) {
        let path = std::path::PathBuf::from(path);
        assert!(path.is_absolute());
        assert!(path.starts_with(std::env::temp_dir()) || path.starts_with("/tmp"));
        assert!(!path.exists(), "never replace an existing fixture receipt");
        std::fs::write(path, bytes).unwrap();
    }
}

#[test]
fn emit_native_installed_md1_fixture() {
    emit(1, "MQ_NATIVE_INSTALLED_TEST_FIXTURE_MD1");
}

#[test]
fn emit_native_installed_md2_fixture() {
    emit(2, "MQ_NATIVE_INSTALLED_TEST_FIXTURE_MD2");
}
