//! Independent fixed SHISAM admission, programming and retained-reader expectations.
use mainframe_env_execution_api::*;
use mainframe_env_host_api::*;
use mainframe_env_ims::{ImsLimits, ImsService, ims_providers};
use mainframe_env_store::{MemoryStore, SqliteStateStore};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

fn catalog() -> ImsMetadataCatalog {
    ImsMetadataCatalog {
        schema_version: IMS_METADATA_SCHEMA_V1.into(),
        databases: vec![ImsDatabaseMetadata {
            gsam_format: None,
            name: "FIXDB".into(),
            version: 1,
            organization: ImsDatabaseOrganization::Shisam,
            segments: vec![ImsSegmentMetadata {
                name: "ROOT".into(),
                parent: None,
                min_length: 3,
                max_length: 3,
                fields: vec![ImsFieldMetadata {
                    name: Some("KEY".into()),
                    offset: 0,
                    length: 2,
                    sequence: true,
                    unique: true,
                }],
            }],
            secondary_indexes: vec![],
            logical_relationships: vec![],
        }],
        psbs: vec![ImsPsbMetadata {
            name: "FIXPSB".into(),
            database_level: ImsDbLevel::Current,
            pcbs: vec![ImsPcbMetadata::Database(ImsDatabasePcbMetadata {
                name: "DBPCB".into(),
                database: "FIXDB".into(),
                database_version: Some(1),
                processing_options: "AP".into(),
                secondary_index: None,
                sensitive_segments: vec![ImsSensitiveSegmentMetadata {
                    name: "ROOT".into(),
                    parent: None,
                    processing_options: None,
                }],
            })],
        }],
    }
}

fn invocation() -> Invocation {
    let limits = InvocationLimits::default();
    Invocation::new(
        RequestId::new("shisam-request", limits).unwrap(),
        ExecutionId::new("shisam-execution", limits).unwrap(),
        RunUnitId::new("shisam-run", limits).unwrap(),
        None,
        Selector::new("ims:shisam", limits).unwrap(),
        ArtifactRef::new("ims:shisam", limits).unwrap(),
        Principal::new(
            PrincipalId::new("FIXUSER", limits).unwrap(),
            ["host.ims.read", "host.ims.write"]
                .into_iter()
                .map(|s| CapabilityId::new(s, limits).unwrap())
                .collect(),
            limits,
        )
        .unwrap(),
        ServiceClass::Batch,
        0,
        100,
        TraceId::new("shisam-trace", limits).unwrap(),
        IdempotencyKey::new("shisam-invocation", limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .unwrap()
}

fn request(operation: ImsOperation, sequence: u64, data: &[u8], key: Option<&[u8]>) -> ImsRequest {
    ImsRequest {
        operation,
        psb: (operation == ImsOperation::Schedule).then(|| "FIXPSB".into()),
        pcb: 1,
        segments: matches!(
            operation,
            ImsOperation::Insert | ImsOperation::GetUnique | ImsOperation::GetHoldUnique
        )
        .then(|| "ROOT".into())
        .into_iter()
        .collect(),
        qualifiers: key
            .map(|key| ImsQualifier {
                segment: "ROOT".into(),
                field: "KEY".into(),
                value: key.to_vec(),
            })
            .into_iter()
            .collect(),
        data: data.to_vec(),
        checkpoint_id: None,
        max_segments: 16,
        mutation: operation.is_mutating().then(|| Mutation {
            sequence,
            idempotency_key: IdempotencyKey::new(
                format!("shisam-{sequence}"),
                InvocationLimits::default(),
            )
            .unwrap(),
            transaction: None,
        }),
        system: None,
        q_class: None,
    }
}

fn make_host(service: Arc<ImsService>) -> ScopedHostService {
    let limits = InvocationLimits::default();
    ScopedHostService::new(
        Arc::new(RegistrySnapshot::new(1, ims_providers(service, limits), limits).unwrap()),
        HostLimits::default(),
    )
}

fn call(
    host: &ScopedHostService,
    store: &dyn ProviderStateStore,
    sequence: u64,
    request: ImsRequest,
) -> ImsResult {
    let inv = invocation();
    let result = host
        .invoke(
            &inv,
            2,
            false,
            EffectRequest {
                run_unit: inv.run_unit_id.clone(),
                sequence,
                idempotency_key: request.mutation.as_ref().map(|m| m.idempotency_key.clone()),
                request: HostRequest::Ims(request),
                deadline_tick: inv.deadline_tick,
            },
        )
        .persist_with(|audit| {
            store
                .record_audit(audit)
                .map_err(|_| HostProblem::InfrastructureFailure)
        });
    match result.outcome.unwrap() {
        HostResult::Ims(result) => result,
        other => panic!("Unexpected public result: {other:?}"),
    }
}

fn rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    store.list_provider_state_prefix("ims", 4096).unwrap()
}

fn sqlite(name: &str) -> (Arc<dyn ProviderStateStore>, PathBuf) {
    let dir = std::env::var_os("SHISAM_RECEIPT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let path = dir.join(format!("shisam-{name}-{}.sqlite", std::process::id()));
    let url = format!("sqlite:{}?mode=rwc", path.display());
    (
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap()),
        path,
    )
}

fn reject_variable(store: Arc<dyn ProviderStateStore>, path: Option<PathBuf>) {
    let good = catalog();
    validate_ims_metadata(&good, ImsMetadataLimits::default()).unwrap();
    let mut invalid = good;
    invalid.databases[0].segments[0].max_length = 4;
    let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    let before = rows(&*store);
    let actual = service.install_metadata(invalid.clone());
    let after_install = rows(&*store);
    eprintln!(
        "SHISAM_PROVIDER_VARIABLE_ACTUAL={actual:?}; BEFORE_ROWS={}; AFTER_ROWS={}",
        before.len(),
        after_install.len()
    );
    // Observe actual legacy bytes through public installation/calls, never fabricated rows.
    if actual.is_ok() {
        let host = make_host(service.clone());
        for (seq, req) in [
            (1, request(ImsOperation::Schedule, 1, &[], None)),
            (2, request(ImsOperation::Insert, 2, b"A1X", None)),
            (3, request(ImsOperation::Insert, 3, b"B2YZ", None)),
            (4, request(ImsOperation::Commit, 4, &[], None)),
        ] {
            let receipt = call(&host, &*store, seq, req);
            eprintln!("SHISAM_LEGACY_CALL_{seq}={receipt:?}");
            assert_eq!(receipt.status, "  ");
        }
        eprintln!(
            "SHISAM_LEGACY_ROWS={}",
            serde_json::to_string(
                &rows(&*store)
                    .iter()
                    .map(|row| (&row.namespace, &row.key, row.version, &row.payload))
                    .collect::<Vec<_>>()
            )
            .unwrap()
        );
        drop(host);
        drop(service);
        let reopened_store: Arc<dyn ProviderStateStore> = if let Some(path) = path {
            let url = format!("sqlite:{}?mode=rw", path.display());
            eprintln!("SHISAM_RETAINED_INVALID_SQLITE={}", path.display());
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap())
        } else {
            store.clone()
        };
        let before_open = rows(&*reopened_store);
        let reopened = ImsService::open(reopened_store.clone(), ImsLimits::default()).unwrap();
        assert_eq!(rows(&*reopened_store), before_open);
        let host = make_host(reopened);
        assert_eq!(
            call(
                &host,
                &*reopened_store,
                5,
                request(ImsOperation::GetUnique, 5, &[], Some(b"B2"))
            )
            .segments[0]
                .data,
            b"B2YZ"
        );
        eprintln!("SHISAM_LEGACY_REOPEN_READABLE=true");
    }
    assert_eq!(actual, Err(HostProblem::Malformed));
    assert_eq!(
        after_install, before,
        "rejected metadata must publish no rows"
    );
}

fn fixed_programming(store: Arc<dyn ProviderStateStore>) {
    let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    let metadata = catalog();
    let identity = service.install_metadata(metadata.clone()).unwrap();
    let host = make_host(service.clone());
    assert_eq!(
        call(
            &host,
            &*store,
            1,
            request(ImsOperation::Schedule, 1, &[], None)
        )
        .status,
        "  "
    );
    let first = request(ImsOperation::Insert, 2, b"B2Y", None);
    let receipt = call(&host, &*store, 2, first.clone());
    assert_eq!(receipt.status, "  ");
    assert_eq!(receipt.affected_segments, 1);
    assert_eq!(
        call(
            &host,
            &*store,
            3,
            request(ImsOperation::Insert, 3, b"A1X", None)
        )
        .affected_segments,
        1
    );
    assert_eq!(
        call(
            &host,
            &*store,
            4,
            request(ImsOperation::Commit, 4, &[], None)
        )
        .status,
        "  "
    );
    for (sequence, operation) in [
        (5, ImsOperation::GetUnique),
        (6, ImsOperation::GetHoldUnique),
    ] {
        let got = call(
            &host,
            &*store,
            sequence,
            request(operation, sequence, &[], Some(b"A1")),
        );
        assert_eq!(got.status, "  ");
        assert_eq!(got.segments[0].data, b"A1X");
    }
    let before_replay = rows(&*store);
    assert_eq!(call(&host, &*store, 2, first.clone()), receipt);
    assert_eq!(rows(&*store), before_replay);
    assert_eq!(
        call(
            &host,
            &*store,
            7,
            request(ImsOperation::GetNext, 7, &[], None)
        )
        .segments[0]
            .data,
        b"B2Y"
    );
    let good_bytes = rows(&*store);
    drop(host);
    drop(service);
    let reopened = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    assert_eq!(
        rows(&*store),
        good_bytes,
        "good retained images must stay byte-exact on reopen"
    );
    assert_eq!(reopened.install_metadata(metadata).unwrap(), identity);
    let host = make_host(reopened);
    let before = rows(&*store);
    assert_eq!(call(&host, &*store, 2, first), receipt);
    assert_eq!(rows(&*store), before);
    assert_eq!(
        call(
            &host,
            &*store,
            8,
            request(ImsOperation::GetUnique, 8, &[], Some(b"A1"))
        )
        .segments[0]
            .data,
        b"A1X"
    );
    assert_eq!(
        call(
            &host,
            &*store,
            9,
            request(ImsOperation::GetNext, 9, &[], None)
        )
        .segments[0]
            .data,
        b"B2Y"
    );
    eprintln!("SHISAM_FIXED_PUBLIC_PROGRAMMING_REPLAY_REOPEN_PASS");
}

#[test]
fn shisam_fixed_layout_provider_rejects_variable_memory() {
    reject_variable(Arc::new(MemoryStore::new(Default::default())), None);
}

#[test]
fn shisam_fixed_layout_provider_rejects_variable_sqlite() {
    let (store, path) = sqlite("legacy-invalid");
    reject_variable(store, Some(path));
}

#[test]
fn shisam_fixed_layout_provider_fixed_programming_memory() {
    fixed_programming(Arc::new(MemoryStore::new(Default::default())));
}

#[test]
fn shisam_fixed_layout_provider_fixed_programming_sqlite() {
    let (store, path) = sqlite("fixed-control");
    fixed_programming(store);
    if std::env::var_os("SHISAM_CAPTURE_GOOD_DIR").is_some() {
        eprintln!("SHISAM_CAPTURE_GOOD_PUBLIC={}", path.display());
    } else {
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
#[ignore = "requires exact externally retained public SQLite state"]
fn shisam_fixed_layout_provider_historical_reader() {
    let path = std::path::PathBuf::from(std::env::var_os("SHISAM_PUBLIC_READER_DB").unwrap());
    let url = format!("sqlite:{}?mode=rw", path.display());
    let store: Arc<dyn ProviderStateStore> =
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    let before = rows(&*store);
    let actual = ImsService::open(store.clone(), ImsLimits::default());
    if std::env::var("SHISAM_PUBLIC_LAYOUT").unwrap() == "good" {
        actual.unwrap();
        let row = store
            .get_provider_state("ims-v1-generic-database", "FIXDB")
            .unwrap()
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        let image = serde_json::from_value(value["value"].clone()).unwrap();
        let engine =
            mainframe_env_ims::database::DatabaseEngine::restore(image, Default::default())
                .unwrap();
        assert_eq!(
            engine
                .export_records()
                .iter()
                .map(|r| r.data.as_slice())
                .collect::<Vec<_>>(),
            vec![&b"B2Y"[..], &b"A1X"[..]]
        );
    } else {
        match std::env::var("SHISAM_READER_EXPECTATION").unwrap().as_str() {
            "old" => {
                actual.unwrap();
            }
            "new" => assert!(matches!(actual, Err(HostProblem::InfrastructureFailure))),
            other => panic!("Invalid reader expectation: {other}"),
        }
    }
    assert_eq!(
        rows(&*store),
        before,
        "reader must preserve every IMS payload and CAS version"
    );
    eprintln!("SHISAM_PUBLIC_HISTORICAL_READER_PASS");
}

#[test]
#[ignore = "requires isolated PostgreSQL18.6 rejection database"]
fn shisam_fixed_layout_provider_rejects_variable_postgres() {
    let url = std::env::var("SHISAM_POSTGRES_URL").unwrap();
    let store =
        mainframe_env_store::PostgresStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
    reject_variable(Arc::new(store), None);
    eprintln!("SHISAM_POSTGRES_PUBLIC_REJECTION_PASS");
}

#[test]
#[ignore = "requires isolated PostgreSQL18.6 fixed programming database"]
fn shisam_fixed_layout_provider_fixed_programming_postgres() {
    let url = std::env::var("SHISAM_POSTGRES_URL").unwrap();
    let store =
        mainframe_env_store::PostgresStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
    fixed_programming(Arc::new(store));
    eprintln!("SHISAM_POSTGRES_PUBLIC_PROGRAMMING_PASS");
}
