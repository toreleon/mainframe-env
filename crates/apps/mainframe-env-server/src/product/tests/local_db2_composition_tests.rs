//! Seven genuine store/catalog controls; no synthesized SQLCA or global-complete evidence.
use super::*;
use mainframe_env_application::{SqlColumn, SqlSeedRow, SqlTable};
use mainframe_env_db2::{
    Db2CatalogGeneration, Db2CatalogProjection, Db2ColumnDefinition, Db2ResultEncoding,
    Db2TableDefinition,
};
use mainframe_env_host_api::{Db2HostVariable, Db2Operation, Db2Request};

const TABLE: &str = "LOCAL.CODES";
const DB2_NAMESPACES: [&str; 11] = [
    "db2-state",
    "db2-v1-table",
    "db2-v1-schema",
    "db2-v1-installation",
    "db2-v1-catalog-generation",
    "db2-v1-table-provenance",
    "db2-v1-legacy-snapshot",
    "db2-v1-unit-of-work",
    "db2-v1-cursor",
    "db2-v1-cursor-declaration",
    "db2-v1-replay",
];
fn table() -> Db2TableDefinition {
    Db2TableDefinition {
        name: TABLE.into(),
        columns: vec![
            Db2ColumnDefinition {
                name: "KEY".into(),
                nullable: false,
                max_bytes: 8,
                result_encoding: Db2ResultEncoding::Raw,
                default_value: None,
            },
            Db2ColumnDefinition {
                name: "VALUE".into(),
                nullable: false,
                max_bytes: 64,
                result_encoding: Db2ResultEncoding::Raw,
                default_value: None,
            },
        ],
        primary_key: vec!["KEY".into()],
        foreign_keys: Vec::new(),
        extract: None,
    }
}
fn signed_catalog(
    package: &mut ApplicationPackageV2,
    trust: &HmacSha256PackageTrust,
    generation: u64,
) {
    let schema = table();
    let bytes = serde_json::to_vec(&vec![schema.clone()]).unwrap();
    let sha = format!("sha256:{:x}", Sha256::digest(&bytes));
    package.base.blobs.insert(sha.clone(), bytes.clone());
    package.base.manifest.entries.push(PackageEntry {
        path: "data/db2/catalog".into(),
        kind: EntryKind::Data,
        sha256: sha,
        bytes: bytes.len(),
        depends_on: Vec::new(),
    });
    package.sections.sql_tables = vec![SqlTable {
        name: schema.name,
        columns: schema
            .columns
            .into_iter()
            .map(|column| SqlColumn {
                name: column.name,
                nullable: column.nullable,
            })
            .collect(),
        primary_key: schema.primary_key,
    }];
    package.sections.sql_rows = vec![SqlSeedRow {
        table: TABLE.into(),
        values: BTreeMap::from([
            (
                "KEY".into(),
                if generation == 1 { "ONE" } else { "TWO" }.into(),
            ),
            ("VALUE".into(), "SIGNED".into()),
        ]),
    }];
    resign_package(package, trust);
}
fn catalog_definitions(
    server: &ProductServer,
    trust: &HmacSha256PackageTrust,
    generation: u64,
) -> (ApplicationPackageV2, LocalApplicationDefinition) {
    let (mut package, definition) = definitions(server, trust, generation);
    signed_catalog(&mut package, trust, generation);
    (package, definition)
}
fn install_catalog_local(
    server: &ProductServer,
    trust: &HmacSha256PackageTrust,
    generation: u64,
) -> LocalApplicationReceipt {
    let (package, definition) = catalog_definitions(server, trust, generation);
    let expected = server.install_application_package_v2(&package).unwrap();
    server
        .publish_local_application(&expected, definition)
        .unwrap()
}
fn rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    let mut rows = selected_rows(store);
    for namespace in DB2_NAMESPACES {
        rows.extend(store.list_provider_state(namespace, 1024).unwrap());
    }
    rows.sort_by(|a, b| (&a.namespace, &a.key).cmp(&(&b.namespace, &b.key)));
    rows
}
fn current_projection(store: &dyn ProviderStateStore) -> Db2CatalogProjection {
    let row = store
        .get_provider_state("application-local-selection", "LOCAL-COMPOSE")
        .unwrap()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
    serde_json::from_value(value["db2"].clone()).unwrap()
}
fn catalog(server: &ProductServer, expected: &ApplicationGenerationRecord) -> Db2CatalogGeneration {
    let durable = server.applications_v2.lock().unwrap();
    let selected = durable
        .installer
        .generation(&expected.package, expected.generation, &expected.identity)
        .unwrap()
        .unwrap();
    server
        .decode_application_db2_catalog(&selected)
        .unwrap()
        .unwrap()
}
fn invocation(run: &str) -> Invocation {
    let limits = InvocationLimits::default();
    Invocation::new(
        RequestId::new(format!("request-{run}"), limits).unwrap(),
        ExecutionId::new(format!("execution-{run}"), limits).unwrap(),
        RunUnitId::new(run, limits).unwrap(),
        None,
        Selector::new("program:DB2TEST", limits).unwrap(),
        ArtifactRef::new("artifact", limits).unwrap(),
        Principal::new(
            PrincipalId::new("IBMUSER", limits).unwrap(),
            BTreeSet::from([
                CapabilityId::new("host.db2.read", limits).unwrap(),
                CapabilityId::new("host.db2.write", limits).unwrap(),
            ]),
            limits,
        )
        .unwrap(),
        ServiceClass::Interactive,
        0,
        10_000,
        TraceId::new(format!("trace-{run}"), limits).unwrap(),
        IdempotencyKey::new(format!("invocation-{run}"), limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .unwrap()
}
fn request(
    operation: Db2Operation,
    sequence: u64,
    statement: &str,
    inputs: BTreeMap<String, Db2HostVariable>,
) -> Db2Request {
    Db2Request {
        operation,
        statement: statement.into(),
        cursor: None,
        inputs,
        outputs: Vec::new(),
        max_rows: 64,
        mutation: operation.is_mutating().then(|| Mutation {
            sequence,
            idempotency_key: IdempotencyKey::new(
                format!("db2-test-{sequence}"),
                InvocationLimits::default(),
            )
            .unwrap(),
            transaction: Some("DB2TEST".into()),
        }),
    }
}
fn variable(value: &str) -> Db2HostVariable {
    Db2HostVariable {
        value: value.as_bytes().to_vec(),
        indicator: None,
    }
}
fn edit(service: &Db2Service, run: &str, delete: bool) {
    let invocation = invocation(run);
    let request = if delete {
        request(
            Db2Operation::Delete,
            91,
            "DELETE FROM LOCAL.CODES",
            BTreeMap::from([("KEY".into(), variable("ONE"))]),
        )
    } else {
        request(
            Db2Operation::Insert,
            91,
            "INSERT INTO LOCAL.CODES",
            BTreeMap::from([
                ("KEY".into(), variable("RUNTIME")),
                ("VALUE".into(), variable("UNTOUCHED")),
            ]),
        )
    };
    let result = service.execute(&invocation, &request).unwrap();
    assert_eq!(result.sqlcode, 0);
    assert_eq!(result.affected_rows, 1);
    assert_eq!(
        service
            .execute(
                &invocation,
                &self::request(Db2Operation::Commit, 92, "COMMIT", BTreeMap::new())
            )
            .unwrap()
            .sqlcode,
        0
    );
}

#[test]
fn db2_signed_full_catalog_and_ready_reopen_share_genuine_memory_rows() {
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = fixture(store.clone(), trust.clone());
    let installed = install_catalog_local(&server, &trust, 1);
    assert_eq!(server.db2.table_definition(TABLE).unwrap(), table());
    assert_eq!(
        server.db2.table_rows(TABLE).unwrap(),
        vec![vec![b"ONE".to_vec(), b"SIGNED".to_vec()]]
    );
    let projection = current_projection(store.as_ref());
    assert_eq!(projection.application, "LOCAL-COMPOSE");
    assert_eq!(projection.generation, 1);
    assert_eq!(projection.identity, installed.generation.identity);
    assert_eq!(projection.tables, BTreeMap::from([(TABLE.into(), table())]));
    let full = rows(store.as_ref());
    drop(server);
    let reopened = fixture(store.clone(), trust);
    assert_eq!(rows(store.as_ref()), full);
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap(),
        Some(installed.generation)
    );
}

#[test]
fn db2_real_runtime_edit_retry_and_retained_rollback_preserve_full_rows() {
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = fixture(store.clone(), trust.clone());
    let first = install_catalog_local(&server, &trust, 1);
    drop(server);
    let service = Db2Service::open(store.clone(), Default::default()).unwrap();
    edit(&service, "runtime", false);
    drop(service);
    let server = fixture(store.clone(), trust.clone());
    let before = rows(store.as_ref());
    let (_, definition) = catalog_definitions(&server, &trust, 1);
    let retry = server
        .publish_local_application(&first.generation, definition)
        .unwrap();
    assert!(retry.replayed);
    assert_eq!(rows(store.as_ref()), before);
    let second = install_catalog_local(&server, &trust, 2);
    assert_eq!(second.generation.generation, 2);
    let (_, definition) = catalog_definitions(&server, &trust, 1);
    server
        .rollback_local_application(&first.generation, definition)
        .unwrap();
    assert_eq!(
        server.db2.table_rows(TABLE).unwrap(),
        vec![
            vec![b"ONE".to_vec(), b"SIGNED".to_vec()],
            vec![b"RUNTIME".to_vec(), b"UNTOUCHED".to_vec()]
        ]
    );
    let full = rows(store.as_ref());
    drop(server);
    let reopened = fixture(store.clone(), trust);
    assert_eq!(rows(store.as_ref()), full);
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap()
            .unwrap()
            .generation,
        1
    );
}

fn foreign_current<S: PlatformStore + 'static>(store: Arc<S>) {
    let trust = Arc::new(test_package_trust());
    let server = fixture(store.clone(), trust.clone());
    let installed = install_catalog_local(&server, &trust, 1);
    drop(server);
    let service = Db2Service::open(store.clone(), Default::default()).unwrap();
    edit(&service, "foreign-empty", true);
    assert!(service.table_rows(TABLE).unwrap().is_empty());
    let before = rows(store.as_ref());
    let foreign = Db2CatalogGeneration {
        application: "FOREIGN-COMPOSE".into(),
        generation: 1,
        identity: format!("sha256:{}", "b".repeat(64)),
        tables: vec![table()],
        rows: Vec::new(),
    };
    assert!(matches!(
        service.prepare_catalog_composition(foreign, None, false),
        Err(HostProblem::IdempotencyConflict)
    ));
    assert_eq!(rows(store.as_ref()), before);
    drop(service);
    let reopened = fixture(store.clone(), trust);
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap(),
        Some(installed.generation)
    );
    assert!(reopened.db2.table_rows(TABLE).unwrap().is_empty());
}
#[test]
fn db2_foreign_current_catalog_still_owns_runtime_empty_tables_on_both_stores() {
    foreign_current(Arc::new(MemoryStore::new(Default::default())));
    static SERIAL: AtomicU64 = AtomicU64::new(1);
    let root = std::env::temp_dir().join(format!(
        "foreign-db2-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir(&root).unwrap();
    let url = format!("sqlite://{}?mode=rwc", root.join("state.sqlite").display());
    foreign_current(Arc::new(
        SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap(),
    ));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn db2_signed_section_full_schema_seed_and_missing_catalog_refusals_do_not_mutate() {
    for mode in 0..6 {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(MemoryStore::new(Default::default()));
        let server = fixture(store.clone(), trust.clone());
        let (mut package, definition) = catalog_definitions(&server, &trust, 1);
        match mode {
            0 => package.sections.sql_tables[0].columns[0].nullable = true,
            1 => package.sections.sql_tables[0].primary_key = vec!["VALUE".into()],
            2 => {
                let entry = package
                    .base
                    .manifest
                    .entries
                    .iter_mut()
                    .find(|entry| entry.path == "data/db2/catalog")
                    .unwrap();
                let mut schema = table();
                schema.columns[0].max_bytes = 0;
                let raw = serde_json::to_vec(&vec![schema]).unwrap();
                entry.sha256 = format!("sha256:{:x}", Sha256::digest(&raw));
                entry.bytes = raw.len();
                package.base.blobs.insert(entry.sha256.clone(), raw);
            }
            3 => {
                package.sections.sql_rows[0]
                    .values
                    .insert("VALUE".into(), "X".repeat(65));
            }
            4 => {
                package
                    .base
                    .manifest
                    .entries
                    .retain(|entry| entry.path != "data/db2/catalog");
            }
            _ => {
                package.sections.sql_tables[0].name = "FOREIGN.CODES".into();
                package.sections.sql_rows[0].table = "FOREIGN.CODES".into();
            }
        }
        let used = package
            .base
            .manifest
            .entries
            .iter()
            .map(|entry| entry.sha256.clone())
            .collect::<BTreeSet<_>>();
        package.base.blobs.retain(|sha, _| used.contains(sha));
        resign_package(&mut package, &trust);
        let expected = server.install_application_package_v2(&package).unwrap();
        let before = rows(store.as_ref());
        assert!(
            server
                .publish_local_application(&expected, definition)
                .is_err()
        );
        assert_eq!(rows(store.as_ref()), before);
        assert_eq!(
            server.selected_local_application("LOCAL-COMPOSE").unwrap(),
            None
        );
    }
}

#[test]
fn db2_unknown_shared_commit_and_wrong_store_acknowledgement_quarantine_until_reopen() {
    for outcome in [Outcome::Before, Outcome::After] {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(FaultStore::new(Arc::new(MemoryStore::new(
            Default::default(),
        ))));
        let server = fixture(store.clone(), trust.clone());
        let (package, definition) = catalog_definitions(&server, &trust, 1);
        let expected = server.install_application_package_v2(&package).unwrap();
        let before = rows(store.as_ref());
        store.arm(outcome);
        assert_eq!(
            server.publish_local_application(&expected, definition),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(
            server.db2.table_rows(TABLE),
            Err(HostProblem::UnknownOutcome)
        );
        drop(server);
        let reopened = fixture(store.clone(), trust);
        if matches!(outcome, Outcome::Before) {
            assert_eq!(rows(store.as_ref()), before);
            assert_eq!(
                reopened
                    .selected_local_application("LOCAL-COMPOSE")
                    .unwrap(),
                None
            );
        } else {
            assert!(
                reopened
                    .selected_local_application("LOCAL-COMPOSE")
                    .unwrap()
                    .is_some()
            );
        }
    }
    let left = Arc::new(MemoryStore::new(Default::default()));
    let right = Arc::new(MemoryStore::new(Default::default()));
    let service = Db2Service::open(left.clone(), Default::default()).unwrap();
    let offered = Db2CatalogGeneration {
        application: "LOCAL-COMPOSE".into(),
        generation: 1,
        identity: format!("sha256:{}", "a".repeat(64)),
        tables: vec![table()],
        rows: Vec::new(),
    };
    let prepared = service
        .prepare_catalog_composition(offered, None, false)
        .unwrap();
    let commit = ProviderCompositionCommit::publish(right, prepared.writes().to_vec()).unwrap();
    assert_eq!(prepared.finish(&commit), Err(HostProblem::UnknownOutcome));
    assert!(rows(left.as_ref()).is_empty());
    assert_eq!(service.table_rows(TABLE), Err(HostProblem::UnknownOutcome));
}

#[test]
fn db2_sqlite_atomic_companion_conflict_preserves_original_full_catalog_and_selection() {
    static SERIAL: AtomicU64 = AtomicU64::new(1);
    let root = std::env::temp_dir().join(format!(
        "db2-cas-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir(&root).unwrap();
    let url = format!("sqlite://{}?mode=rwc", root.join("state.sqlite").display());
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    let server = fixture(store.clone(), trust.clone());
    let first = install_catalog_local(&server, &trust, 1);
    let (package, definition) = catalog_definitions(&server, &trust, 2);
    let expected = server.install_application_package_v2(&package).unwrap();
    let previous = current_projection(store.as_ref());
    let offered = catalog(&server, &expected);
    let companion = ProviderStateRecord {
        namespace: "db2-composition-companion".into(),
        key: "owned".into(),
        version: 1,
        payload: b"UNCHANGED".to_vec(),
    };
    store.put_provider_state(companion.clone(), None).unwrap();
    let before = rows(store.as_ref());
    let prepared = server
        .db2
        .prepare_catalog_composition(offered, Some(&previous), false)
        .unwrap();
    let mut writes = prepared.writes().to_vec();
    writes.push(ProviderStateWrite {
        record: companion.clone(),
        expected_version: None,
    });
    assert!(ProviderCompositionCommit::publish(store.clone(), writes).is_err());
    drop(prepared);
    assert_eq!(rows(store.as_ref()), before);
    assert_eq!(
        store
            .get_provider_state(&companion.namespace, &companion.key)
            .unwrap(),
        Some(companion)
    );
    server
        .publish_local_application(&expected, definition)
        .unwrap();
    let (_, definition) = catalog_definitions(&server, &trust, 1);
    server
        .rollback_local_application(&first.generation, definition)
        .unwrap();
    let full = rows(store.as_ref());
    drop(server);
    let reopened = fixture(store.clone(), trust);
    assert_eq!(rows(store.as_ref()), full);
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap()
            .unwrap()
            .generation,
        1
    );
    drop(reopened);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn db2_real_busy_capacity_stale_projection_and_store_version_refuse_without_writes() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let trust = Arc::new(test_package_trust());
    let server = fixture(store.clone(), trust.clone());
    let installed = install_catalog_local(&server, &trust, 1);
    let mut projection = current_projection(store.as_ref());
    projection.tables.get_mut(TABLE).unwrap().columns[1].max_bytes = 1;
    let before = rows(store.as_ref());
    assert_eq!(
        server.db2.selected_catalog_composition_matches(&projection),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(rows(store.as_ref()), before);
    let offered = catalog(&server, &installed.generation);
    drop(server);
    let service = Db2Service::open(store.clone(), Default::default()).unwrap();
    let invocation = invocation("busy");
    let insert = request(
        Db2Operation::Insert,
        101,
        "INSERT INTO LOCAL.CODES",
        BTreeMap::from([
            ("KEY".into(), variable("PENDING")),
            ("VALUE".into(), variable("HELD")),
        ]),
    );
    assert_eq!(service.execute(&invocation, &insert).unwrap().sqlcode, 0);
    let before = rows(store.as_ref());
    assert!(matches!(
        service.prepare_catalog_composition(
            offered,
            Some(&current_projection(store.as_ref())),
            false
        ),
        Err(HostProblem::Condition { response: -904, .. })
    ));
    assert_eq!(rows(store.as_ref()), before);
    let limited = Db2Service::open(
        Arc::new(MemoryStore::new(Default::default())),
        mainframe_env_db2::Db2Limits {
            max_tables: 0,
            ..Default::default()
        },
    )
    .unwrap();
    let offered = Db2CatalogGeneration {
        application: "LOCAL-COMPOSE".into(),
        generation: 1,
        identity: format!("sha256:{}", "a".repeat(64)),
        tables: vec![table()],
        rows: Vec::new(),
    };
    assert!(matches!(
        limited.prepare_catalog_composition(offered, None, false),
        Err(HostProblem::Malformed)
    ));
    // Actual same-payload provider version drift must not be silently refreshed from an unrelated writer.
    let actual = store
        .get_provider_state("db2-v1-schema", TABLE)
        .unwrap()
        .unwrap();
    store
        .put_provider_state(
            ProviderStateRecord {
                version: actual.version + 1,
                ..actual.clone()
            },
            Some(actual.version),
        )
        .unwrap();
    let before = rows(store.as_ref());
    assert_eq!(
        service.selected_catalog_composition_matches(&current_projection(store.as_ref())),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(rows(store.as_ref()), before);
}
