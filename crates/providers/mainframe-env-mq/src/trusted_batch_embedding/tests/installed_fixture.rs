//! Reproducible setup only. Uses the actual quiescent import plan; zero execution
//! or SAF/oracle credit. No product normalization API is exported.
use super::*;
#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct SetupRow {
    namespace: String,
    key: String,
    version: u64,
    payload: Vec<u8>,
}
#[test]
fn emit_configured_installed_rich_fixture() {
    let store = backend(false);
    let capture = || {
        store
            .list_provider_state_prefix("mq-", 4096)
            .unwrap()
            .into_iter()
            .map(|r| SetupRow {
                namespace: r.namespace,
                key: r.key,
                version: r.version,
                payload: r.payload,
            })
            .collect::<Vec<_>>()
    };
    let legacy = MqService::open(store.clone(), Default::default()).unwrap();
    let mut rows = vec![capture()];
    legacy
        .install(vec![MqQueueDefinition {
            name: "Q".into(),
            trigger_program: None,
        }])
        .unwrap();
    rows.push(capture());
    let plan = legacy
        .plan_legacy_delivery_import(3, 5, Default::default())
        .unwrap();
    store
        .mutate_provider_states_atomic(plan.into_parts().0)
        .unwrap();
    rows.push(capture());
    let mut bytes = serde_json::to_vec_pretty(&rows).unwrap();
    bytes.push(b'\n');
    let decoded: Vec<Vec<SetupRow>> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded, rows);
    if let Some(path) = std::env::var_os("MQ_INSTALLED_TEST_FIXTURE") {
        let path = std::path::PathBuf::from(path);
        assert!(path.is_absolute());
        assert!(path.starts_with(std::env::temp_dir()) || path.starts_with("/tmp"));
        std::fs::write(path, bytes).unwrap();
    }
}
