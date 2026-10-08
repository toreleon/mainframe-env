//! Physical SQLite provider-state close/reopen, not IBM warm restart or route admission.
//! Artifacts deliberately remain in a separate MemoryStore throughout these tests.

use super::*;
use mainframe_env_store::SqliteStateStore;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFINITION_NAMESPACE: &str = "cics-program-definition-v1";
const SCAN_BOUND: usize = 64;

struct DatabaseFixture(PathBuf);

impl DatabaseFixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "mainframe-env-program-status-sqlite-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn open(&self) -> Arc<SqliteStateStore> {
        Arc::new(
            SqliteStateStore::open(
                &format!(
                    "sqlite://{}?mode=rwc",
                    self.0.join("state.sqlite").display()
                ),
                4 * 1024 * 1024,
                SCAN_BOUND,
            )
            .unwrap(),
        )
    }
}

impl Drop for DatabaseFixture {
    fn drop(&mut self) {
        let result = std::fs::remove_dir_all(&self.0);
        if !std::thread::panicking() {
            result.unwrap();
            assert!(!self.0.exists());
            eprintln!("PROGRAM SQLite fixture removed: {}", self.0.display());
        }
    }
}

fn open_service(store: Arc<SqliteStateStore>) -> Result<Arc<CicsService>, HostProblem> {
    // No host provider (including a loader) is registered in this observation fixture.
    let host = Arc::new(ScopedHostService::new(
        Arc::new(RegistrySnapshot::new(1, vec![], InvocationLimits::default()).unwrap()),
        HostLimits::default(),
    ));
    CicsService::open(host, store, crate::service::CicsLimits::default())
}

fn local_definition(
    artifacts: &MemoryStore,
    name: &str,
    generation: u64,
    enabled: bool,
) -> CicsProgramDefinition {
    let mut value = definition(artifacts, name, generation, enabled);
    value.remote = false;
    value.java_status = super::super::super::CicsJavaStatus::NotJava;
    value
}

fn close_last_owner(cics: Arc<CicsService>, store: Arc<SqliteStateStore>) {
    assert_eq!(Arc::strong_count(&cics), 1);
    let old_service = Arc::downgrade(&cics);
    let old_store = Arc::downgrade(&store);
    drop(cics);
    assert!(old_service.upgrade().is_none());
    assert_eq!(Arc::strong_count(&store), 1);
    drop(store);
    assert!(old_store.upgrade().is_none(), "SQLite owner survived close");
    eprintln!("PROGRAM SQLite last service/store Arc dropped before fresh open");
}

fn provider_bytes(store: &SqliteStateStore) -> Vec<ProviderStateRecord> {
    let rows = store
        .list_provider_state_prefix("cics-", SCAN_BOUND)
        .unwrap();
    assert!(
        rows.len() < SCAN_BOUND,
        "snapshot must not silently truncate"
    );
    rows
}

fn application_entry(
    value: &CicsProgramDefinition,
) -> super::super::super::CicsApplicationEntryDefinition {
    super::super::super::CicsApplicationEntryDefinition {
        application: "APP".into(),
        platform: "PLATFORM".into(),
        major_version: 1,
        minor_version: 0,
        micro_version: 0,
        operation: "ENTRY".into(),
        program: value.name.clone(),
        program_generation: value.generation,
        program_artifact: value.artifact.clone(),
        application_identity: format!("sha256:{:064x}", 1),
        available: true,
    }
}

#[test]
fn stable_catalog_and_immutable_references_survive_last_sqlite_owner_close() {
    let database = DatabaseFixture::new();
    let artifacts = Arc::new(MemoryStore::new(Default::default()));
    let store = database.open();
    let cics = open_service(store.clone()).unwrap();
    cics.bind_artifact_store(artifacts.clone()).unwrap();
    let enabled = local_definition(&artifacts, "ONPGM", 3, true);
    let disabled = local_definition(&artifacts, "OFFPGM", 9, false);
    let old = local_definition(&artifacts, "VERSIONS", 2, true);
    let middle = local_definition(&artifacts, "VERSIONS", 7, true);
    let latest = local_definition(&artifacts, "VERSIONS", 11, false);
    // Register out of generation order; observation must still select the latest whole value.
    let definitions = [
        enabled.clone(),
        disabled.clone(),
        old.clone(),
        latest.clone(),
        middle.clone(),
    ];
    cics.register_program_definitions(&definitions).unwrap();
    cics.register_programs(&BTreeSet::from(["LEGACY".into()]))
        .unwrap();
    let entry = application_entry(&old);
    cics.register_application_entries(std::slice::from_ref(&entry))
        .unwrap();
    let saved = observe_named_status(&cics, "VERSIONS").unwrap();
    assert_eq!(saved, ProgramStatusObservation::Definition(latest.clone()));
    let before = provider_bytes(&store);
    assert_eq!(before.len(), 10); // five generations, four name rows, one application entry
    assert_eq!(
        store
            .list_provider_state(DEFINITION_NAMESPACE, SCAN_BOUND)
            .unwrap()
            .len(),
        5
    );
    let artifact_records = definitions
        .iter()
        .map(|d| artifacts.get_artifact(&d.artifact).unwrap().unwrap())
        .collect::<Vec<_>>();
    close_last_owner(cics, store);

    let store = database.open();
    assert_eq!(provider_bytes(&store), before);
    let reopened = open_service(store.clone()).unwrap();
    reopened.bind_artifact_store(artifacts.clone()).unwrap();
    assert_eq!(
        provider_bytes(&store),
        before,
        "service open rewrote the catalog"
    );
    let expected_catalog = BTreeMap::from([
        ("ONPGM".into(), BTreeMap::from([(3, enabled.clone())])),
        ("OFFPGM".into(), BTreeMap::from([(9, disabled.clone())])),
        (
            "VERSIONS".into(),
            BTreeMap::from([(2, old.clone()), (7, middle), (11, latest.clone())]),
        ),
    ]);
    let expected_names = BTreeSet::from([
        "ONPGM".into(),
        "OFFPGM".into(),
        "VERSIONS".into(),
        "LEGACY".into(),
    ]);
    for _ in 0..3 {
        for value in [&enabled, &disabled, &latest] {
            assert_eq!(
                observe_named_status(&reopened, &value.name),
                Ok(ProgramStatusObservation::Definition(value.clone()))
            );
        }
        assert_eq!(
            observe_named_status(&reopened, "LEGACY"),
            Ok(ProgramStatusObservation::NameOnly)
        );
        assert_eq!(
            observe_named_status(&reopened, "MISSING"),
            Ok(ProgramStatusObservation::NotCatalogued)
        );
    }
    assert_eq!(saved, ProgramStatusObservation::Definition(latest));
    {
        let state = reopened.lock().unwrap();
        assert_eq!(state.program_definitions, expected_catalog);
        assert_eq!(state.programs, expected_names);
        assert_eq!(state.application_entries, vec![entry]);
        assert!(state.program_loads.is_empty());
        assert!(state.runs.is_empty());
        assert_eq!(state.application_entries[0].program_artifact, old.artifact);
        assert_eq!(state.application_entries[0].program_generation, 2);
    }
    assert_eq!(
        provider_bytes(&store),
        before,
        "observation mutated provider bytes/version/keys"
    );
    for expected in artifact_records {
        assert_eq!(
            artifacts.get_artifact(&expected.artifact).unwrap(),
            Some(expected)
        );
    }
    close_last_owner(reopened, store);
}

#[derive(Clone, Copy, Debug)]
enum Damage {
    MalformedFlag,
    Truncated,
    CatalogKey,
    DanglingApplication,
}

fn corrupt_then_physically_reopen(damage: Damage) {
    let database = DatabaseFixture::new();
    let artifacts = Arc::new(MemoryStore::new(Default::default()));
    let store = database.open();
    let cics = open_service(store.clone()).unwrap();
    cics.bind_artifact_store(artifacts.clone()).unwrap();
    let value = local_definition(&artifacts, "CORRUPT", 2, true);
    cics.register_program_definitions(std::slice::from_ref(&value))
        .unwrap();
    if matches!(damage, Damage::DanglingApplication) {
        cics.register_application_entries(&[application_entry(&value)])
            .unwrap();
    }
    let mut row = store
        .list_provider_state(DEFINITION_NAMESPACE, SCAN_BOUND)
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(row.version, 1);
    assert!(row.payload.starts_with(b"MECPGD1"));
    let original_payload = row.payload.clone();
    // Fixture corruption through the existing durable API, retaining version 1 to isolate
    // payload/key/catalog consistency from the separate immutable-row version check.
    store
        .delete_provider_state(&row.namespace, &row.key, row.version)
        .unwrap();
    match damage {
        Damage::MalformedFlag => {
            let enabled_flag = row.payload.len() - 4;
            assert_eq!(row.payload[enabled_flag], 1);
            row.payload[enabled_flag] = 2; // neither encoded false nor true
        }
        Damage::Truncated => {
            row.payload.pop().unwrap();
        }
        Damage::CatalogKey => {
            row.key = "OTHER:00000000000000000002".into();
        }
        Damage::DanglingApplication => {}
    }
    if !matches!(damage, Damage::DanglingApplication) {
        store.put_provider_state(row.clone(), None).unwrap();
        let durable = store
            .get_provider_state(&row.namespace, &row.key)
            .unwrap()
            .unwrap();
        assert_eq!(durable, row);
        if matches!(damage, Damage::CatalogKey) {
            assert_eq!(durable.payload, original_payload);
        }
    }
    assert!(
        store
            .get_provider_state("cics-program", "CORRUPT")
            .unwrap()
            .is_some(),
        "NameOnly fallback must remain available but must not mask corrupt typed state"
    );
    let before = provider_bytes(&store);
    close_last_owner(cics, store);
    let store = database.open();
    assert_eq!(provider_bytes(&store), before);
    let result = open_service(store.clone());
    assert!(
        matches!(result, Err(HostProblem::InfrastructureFailure)),
        "{damage:?}: corrupt durable state admitted a service or fell back to NameOnly"
    );
    assert_eq!(
        provider_bytes(&store),
        before,
        "failed open repaired/discarded corrupt state"
    );
    let weak = Arc::downgrade(&store);
    assert_eq!(Arc::strong_count(&store), 1);
    drop(store);
    assert!(weak.upgrade().is_none());
    eprintln!("PROGRAM SQLite {damage:?}: fail-closed after last-owner close");
}

#[test]
fn malformed_durable_mecpgd1_flag_fails_closed_after_sqlite_reopen() {
    corrupt_then_physically_reopen(Damage::MalformedFlag);
}

#[test]
fn truncated_durable_mecpgd1_fails_closed_after_sqlite_reopen() {
    corrupt_then_physically_reopen(Damage::Truncated);
}

#[test]
fn definition_catalog_key_mismatch_fails_closed_after_sqlite_reopen() {
    corrupt_then_physically_reopen(Damage::CatalogKey);
}

#[test]
fn dangling_application_catalog_reference_fails_closed_after_sqlite_reopen() {
    corrupt_then_physically_reopen(Damage::DanglingApplication);
}
