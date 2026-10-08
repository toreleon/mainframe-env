use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(1);

enum Fixture {
    Memory(Arc<MemoryStore>),
    Sqlite(PathBuf),
}

impl Fixture {
    fn memory() -> Self {
        Self::Memory(Arc::new(MemoryStore::new(Default::default())))
    }

    fn sqlite() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-db2-catalog-generation-{}-{}",
            std::process::id(),
            DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        Self::Sqlite(directory)
    }

    fn open(&self) -> Arc<dyn ProviderStateStore> {
        match self {
            Self::Memory(store) => store.clone(),
            Self::Sqlite(directory) => Arc::new(
                SqliteStateStore::open(
                    &format!("sqlite://{}?mode=rwc", directory.join("state.db").display()),
                    64 * 1024 * 1024,
                    262_144,
                )
                .unwrap(),
            ),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Self::Sqlite(directory) = self {
            std::fs::remove_dir_all(directory).unwrap();
        }
    }
}

fn catalog(generation: u64, identity: u8) -> Db2CatalogGeneration {
    let mut catalog = installed_catalog(identity);
    catalog.generation = generation;
    catalog.tables.truncate(1);
    catalog.rows.truncate(1);
    catalog
}

fn observations(service: &Db2Service) -> (State, RowVersions, Vec<ProviderStateRecord>) {
    let durable = service.lock().unwrap();
    (
        durable.state.clone(),
        durable.versions.clone(),
        service
            .store
            .list_provider_state_prefix("db2-", 256)
            .unwrap(),
    )
}

fn assert_selected(service: &Db2Service, generation: u64, identity: u8) {
    let durable = service.lock().unwrap();
    let selected = &durable.state.installations["GENERIC-FIXTURE"];
    assert_eq!(selected.generation, generation);
    assert_eq!(selected.identity, format!("sha256:{identity:064x}"));
}

fn assert_original_row(service: &Db2Service) {
    assert_eq!(
        service.table_rows("APP.CODE").unwrap(),
        vec![vec![b"01".to_vec(), b"GENERIC".to_vec()]]
    );
}

fn reopen(fixture: &Fixture, service: Arc<Db2Service>, limits: Db2Limits) -> Arc<Db2Service> {
    drop(service);
    Db2Service::open(fixture.open(), limits).unwrap()
}

fn seed_legacy(service: &Db2Service) {
    service
        .execute(
            &invocation("catalog-capacity-legacy"),
            &request(
                Db2Operation::ExecuteScript,
                110,
                "CREATE TABLE APP.CODE (CODE CHAR(2) NOT NULL, DESCRIPTION VARCHAR(50) NOT NULL, PRIMARY KEY (CODE)); INSERT INTO APP.CODE (CODE,DESCRIPTION) VALUES ('01','GENERIC');",
                BTreeMap::new(),
            ),
        )
        .unwrap();
    assert_original_row(service);
}

fn initial_adoption_accepts_identical_seed_at_capacity(fixture: Fixture) {
    let limits = Db2Limits {
        max_rows_per_table: 1,
        ..Default::default()
    };
    let service = Db2Service::open(fixture.open(), limits).unwrap();
    seed_legacy(&service);
    let mut first = catalog(1, 1);
    first.tables = vec![service.table_definition("APP.CODE").unwrap()];
    service.install_catalog(first.clone()).unwrap();
    assert_original_row(&service);
    assert_selected(&service, 1, 1);
    let before_retry = observations(&service);
    service.install_catalog(first).unwrap();
    assert_eq!(observations(&service), before_retry);
    let reopened = reopen(&fixture, service, limits);
    assert_eq!(observations(&reopened), before_retry);
    assert_original_row(&reopened);
    assert_selected(&reopened, 1, 1);
}

fn initial_adoption_refuses_changed_and_new_seeds_without_mutation(fixture: Fixture) {
    let limits = Db2Limits {
        max_rows_per_table: 1,
        ..Default::default()
    };
    let service = Db2Service::open(fixture.open(), limits).unwrap();
    seed_legacy(&service);
    let mut first = catalog(1, 1);
    first.tables = vec![service.table_definition("APP.CODE").unwrap()];
    let before = observations(&service);
    let mut changed = first.clone();
    changed.rows[0]
        .values
        .insert("DESCRIPTION".into(), b"CHANGED".to_vec());
    assert_eq!(
        service.install_catalog(changed),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(observations(&service), before);
    first.rows[0].values.insert("CODE".into(), b"02".to_vec());
    assert_eq!(
        service.install_catalog(first),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(observations(&service), before);
    let reopened = reopen(&fixture, service, limits);
    assert_eq!(observations(&reopened), before);
    assert_original_row(&reopened);
    assert!(reopened.lock().unwrap().state.installations.is_empty());
}

fn upgrade_accepts_identical_seed_at_capacity(fixture: Fixture) {
    let limits = Db2Limits {
        max_rows_per_table: 1,
        ..Default::default()
    };
    let service = Db2Service::open(fixture.open(), limits).unwrap();
    service.install_catalog(catalog(1, 1)).unwrap();
    let second = catalog(2, 2);
    service.install_catalog(second.clone()).unwrap();
    assert_original_row(&service);
    assert_selected(&service, 2, 2);
    let before_retry = observations(&service);
    service.install_catalog(second).unwrap();
    assert_eq!(observations(&service), before_retry);
    assert_eq!(
        service.install_catalog(catalog(1, 1)),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(observations(&service), before_retry);
    let reopened = reopen(&fixture, service, limits);
    assert_eq!(observations(&reopened), before_retry);
    assert_selected(&reopened, 2, 2);
    assert_original_row(&reopened);
    reopened.rollback_catalog("GENERIC-FIXTURE", 1).unwrap();
    assert_selected(&reopened, 1, 1);
    assert_original_row(&reopened);
}

fn upgrade_refuses_changed_and_new_seeds_without_mutation(fixture: Fixture) {
    let limits = Db2Limits {
        max_rows_per_table: 1,
        ..Default::default()
    };
    let service = Db2Service::open(fixture.open(), limits).unwrap();
    service.install_catalog(catalog(1, 1)).unwrap();
    let before = observations(&service);
    let mut changed = catalog(2, 2);
    changed.rows[0]
        .values
        .insert("DESCRIPTION".into(), b"CHANGED".to_vec());
    assert_eq!(
        service.install_catalog(changed),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(observations(&service), before);
    let mut added = catalog(2, 2);
    added.rows[0].values.insert("CODE".into(), b"02".to_vec());
    assert_eq!(
        service.install_catalog(added),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(observations(&service), before);
    let reopened = reopen(&fixture, service, limits);
    assert_eq!(observations(&reopened), before);
    assert_selected(&reopened, 1, 1);
    assert_original_row(&reopened);
}

fn excess_seed_entries_refuse_without_mutation(fixture: Fixture) {
    let limits = Db2Limits {
        max_rows_per_table: 1,
        ..Default::default()
    };
    let service = Db2Service::open(fixture.open(), limits).unwrap();
    let before = observations(&service);
    let mut first = catalog(1, 1);
    first.rows.push(first.rows[0].clone());
    assert_eq!(
        service.install_catalog(first),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(observations(&service), before);
    let reopened = reopen(&fixture, service, limits);
    assert_eq!(observations(&reopened), before);
    assert!(reopened.lock().unwrap().state.installations.is_empty());
}

fn rollback_preserves_retained_identity_and_identical_retry(fixture: Fixture) {
    let limits = Db2Limits {
        max_rows_per_table: 2,
        ..Default::default()
    };
    let service = Db2Service::open(fixture.open(), limits).unwrap();
    service.install_catalog(catalog(1, 1)).unwrap();
    let mut second = catalog(2, 2);
    second.rows.push(crate::Db2SeedRow {
        table: "APP.CODE".into(),
        values: BTreeMap::from([
            ("CODE".into(), b"02".to_vec()),
            ("DESCRIPTION".into(), b"SECOND".to_vec()),
        ]),
    });
    service.install_catalog(second.clone()).unwrap();
    service.rollback_catalog("GENERIC-FIXTURE", 1).unwrap();
    let service = reopen(&fixture, service, limits);
    assert_selected(&service, 1, 1);
    assert_original_row(&service);
    let before = observations(&service);
    let mut conflicting = second.clone();
    conflicting.identity = format!("sha256:{:064x}", 3);
    conflicting.rows[1]
        .values
        .insert("DESCRIPTION".into(), b"REPLACED".to_vec());
    assert_eq!(
        service.install_catalog(conflicting),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(observations(&service), before);
    let service = reopen(&fixture, service, limits);
    assert_eq!(observations(&service), before);
    service.rollback_catalog("GENERIC-FIXTURE", 2).unwrap();
    assert_selected(&service, 2, 2);
    let expected_rows = vec![
        vec![b"01".to_vec(), b"GENERIC".to_vec()],
        vec![b"02".to_vec(), b"SECOND".to_vec()],
    ];
    assert_eq!(service.table_rows("APP.CODE").unwrap(), expected_rows);
    service.rollback_catalog("GENERIC-FIXTURE", 1).unwrap();
    service.install_catalog(second.clone()).unwrap();
    assert_selected(&service, 2, 2);
    assert_eq!(service.table_rows("APP.CODE").unwrap(), expected_rows);
    let before_retry = observations(&service);
    service.install_catalog(second).unwrap();
    assert_eq!(observations(&service), before_retry);
    let reopened = reopen(&fixture, service, limits);
    assert_eq!(observations(&reopened), before_retry);
    assert_selected(&reopened, 2, 2);
    assert_eq!(reopened.table_rows("APP.CODE").unwrap(), expected_rows);
}

macro_rules! backend_tests {
    ($backend:ident, $($case:ident),+ $(,)?) => {
        mod $backend {
            use super::*;
            $(
                #[test]
                fn $case() {
                    super::$case(Fixture::$backend());
                }
            )+
        }
    };
}

backend_tests!(
    memory,
    initial_adoption_accepts_identical_seed_at_capacity,
    initial_adoption_refuses_changed_and_new_seeds_without_mutation,
    upgrade_accepts_identical_seed_at_capacity,
    upgrade_refuses_changed_and_new_seeds_without_mutation,
    excess_seed_entries_refuse_without_mutation,
    rollback_preserves_retained_identity_and_identical_retry,
);
backend_tests!(
    sqlite,
    initial_adoption_accepts_identical_seed_at_capacity,
    initial_adoption_refuses_changed_and_new_seeds_without_mutation,
    upgrade_accepts_identical_seed_at_capacity,
    upgrade_refuses_changed_and_new_seeds_without_mutation,
    excess_seed_entries_refuse_without_mutation,
    rollback_preserves_retained_identity_and_identical_retry,
);
