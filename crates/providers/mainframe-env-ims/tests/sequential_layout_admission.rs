//! Independent typed sequential layout fail-first witnesses.
use mainframe_env_execution_api::*;
use mainframe_env_host_api::*;
use mainframe_env_ims::{ImsService, ims_providers};
use mainframe_env_store::{MemoryStore, SqliteStateStore};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

fn seq_layout_catalog(case: &str) -> ImsMetadataCatalog {
    let mut value = seq_layout_base_catalog();
    let org = if case.starts_with("hsam") {
        ImsDatabaseOrganization::Hsam
    } else if case.starts_with("shsam") {
        ImsDatabaseOrganization::Shsam
    } else if case.starts_with("hisam") {
        ImsDatabaseOrganization::Hisam
    } else if case.starts_with("hidam") {
        ImsDatabaseOrganization::Hidam
    } else {
        ImsDatabaseOrganization::Shisam
    };
    value.databases[0].organization = org;
    if org == ImsDatabaseOrganization::Hsam || case.ends_with("multiple") {
        let mut child = value.databases[0].segments[0].clone();
        child.name = "CHILD".into();
        child.parent = Some("ROOT".into());
        value.databases[0].segments.push(child);
        let ImsPcbMetadata::Database(pcb) = &mut value.psbs[0].pcbs[0] else {
            unreachable!()
        };
        pcb.sensitive_segments.push(ImsSensitiveSegmentMetadata {
            name: "CHILD".into(),
            parent: Some("ROOT".into()),
            processing_options: None,
        });
    }
    if case.contains("variable")
        || matches!(
            org,
            ImsDatabaseOrganization::Hisam | ImsDatabaseOrganization::Hidam
        )
    {
        let index = usize::from(case == "hsam-dependent-variable");
        value.databases[0].segments[index].max_length = 4;
    }
    value
}

fn seq_layout_base_catalog() -> ImsMetadataCatalog {
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

fn seq_layout_invocation() -> Invocation {
    let limits = InvocationLimits::default();
    Invocation::new(
        RequestId::new("seq-layout-seq_layout_request", limits).unwrap(),
        ExecutionId::new("seq-layout-execution", limits).unwrap(),
        RunUnitId::new("seq-layout-run", limits).unwrap(),
        None,
        Selector::new("ims:seq-layout", limits).unwrap(),
        ArtifactRef::new("ims:seq-layout", limits).unwrap(),
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
        TraceId::new("seq-layout-trace", limits).unwrap(),
        IdempotencyKey::new("seq-layout-seq_layout_invocation", limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .unwrap()
}

fn seq_layout_request(
    operation: ImsOperation,
    sequence: u64,
    data: &[u8],
    key: Option<&[u8]>,
) -> ImsRequest {
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
                format!("seq-layout-{sequence}"),
                InvocationLimits::default(),
            )
            .unwrap(),
            transaction: None,
        }),
        system: None,
        q_class: None,
    }
}

fn seq_layout_make_host(service: Arc<ImsService>) -> ScopedHostService {
    let limits = InvocationLimits::default();
    ScopedHostService::new(
        Arc::new(RegistrySnapshot::new(1, ims_providers(service, limits), limits).unwrap()),
        HostLimits::default(),
    )
}

fn seq_layout_call(
    host: &ScopedHostService,
    store: &dyn ProviderStateStore,
    sequence: u64,
    seq_layout_request: ImsRequest,
) -> ImsResult {
    let inv = seq_layout_invocation();
    let result = host
        .invoke(
            &inv,
            2,
            false,
            EffectRequest {
                run_unit: inv.run_unit_id.clone(),
                sequence,
                idempotency_key: seq_layout_request
                    .mutation
                    .as_ref()
                    .map(|m| m.idempotency_key.clone()),
                request: HostRequest::Ims(seq_layout_request),
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

fn seq_layout_rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    // Fresh stores contain only owner-created ASCII namespaces. The API requires
    // a nonempty prefix; scan every non-NUL ASCII first byte, not selected owners.
    let mut rows = Vec::new();
    for first in 1u8..=127 {
        let prefix = char::from(first).to_string();
        let part = store.list_provider_state_prefix(&prefix, 4096).unwrap();
        assert!(part.len() < 4096, "bounded provider scan must not truncate");
        rows.extend(part);
    }
    rows.sort_by(|a, b| (&a.namespace, &a.key).cmp(&(&b.namespace, &b.key)));
    rows
}

fn seq_layout_sqlite(name: &str) -> (Arc<dyn ProviderStateStore>, PathBuf) {
    let dir = std::env::var_os("SEQ_LAYOUT_RECEIPT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let path = dir.join(format!("seq-layout-{name}-{}.sqlite", std::process::id()));
    let url = format!("sqlite:{}?mode=rwc", path.display());
    (
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap()),
        path,
    )
}

fn seq_layout_log_rows(label: &str, store: &dyn ProviderStateStore) {
    eprintln!(
        "SEQ_LAYOUT_{label}_ROWS={}",
        serde_json::to_string(
            &seq_layout_rows(store)
                .iter()
                .map(|r| (&r.namespace, &r.key, r.version, &r.payload))
                .collect::<Vec<_>>()
        )
        .unwrap()
    );
}
fn seq_layout_load(case: &str, sequence: u64) -> ImsRequest {
    use mainframe_env_ims::{ImsGenericLoadImage, ImsGenericLoadRecord};
    let catalog = seq_layout_catalog(case);
    let two = catalog.databases[0].segments.len() == 2;
    let mut records = vec![ImsGenericLoadRecord {
        segment: "ROOT".into(),
        parent: None,
        data: if case == "hsam-root-variable" {
            b"A1XY".to_vec()
        } else {
            b"A1X".to_vec()
        },
    }];
    if two {
        records.push(ImsGenericLoadRecord {
            segment: "CHILD".into(),
            parent: Some(0),
            data: if case == "hsam-dependent-variable" {
                b"C1XY".to_vec()
            } else {
                b"C1Y".to_vec()
            },
        });
    }
    records.push(ImsGenericLoadRecord {
        segment: "ROOT".into(),
        parent: None,
        data: if case == "hsam-root-variable" {
            b"B2YZ".to_vec()
        } else {
            b"B2Y".to_vec()
        },
    });
    let image = ImsGenericLoadImage {
        database: "FIXDB".into(),
        records,
    };
    seq_layout_request(
        ImsOperation::Load,
        sequence,
        &serde_json::to_vec(&image).unwrap(),
        None,
    )
}
fn seq_layout_reject(store: Arc<dyn ProviderStateStore>, case: &str) {
    let metadata = seq_layout_catalog(case);
    let host_validation = validate_ims_metadata(&metadata, Default::default());
    let service = ImsService::open(store.clone(), Default::default()).unwrap();
    let before = seq_layout_rows(&*store);
    let actual = service.install_metadata(metadata.clone());
    let after = seq_layout_rows(&*store);
    eprintln!(
        "SEQ_LAYOUT_PUBLIC_CASE={case}; HOST={host_validation:?}; ACTUAL={actual:?}; BEFORE={}; AFTER={}",
        before.len(),
        after.len()
    );
    eprintln!(
        "SEQ_LAYOUT_PUBLIC_METADATA={}",
        serde_json::to_string(&metadata).unwrap()
    );
    seq_layout_log_rows("AFTER_INSTALL", &*store);
    if actual.is_ok() {
        let host = seq_layout_make_host(service.clone());
        for (sequence, request) in [
            (1, seq_layout_request(ImsOperation::Schedule, 1, &[], None)),
            (2, seq_layout_load(case, 2)),
            (3, seq_layout_request(ImsOperation::Commit, 3, &[], None)),
        ] {
            let receipt = seq_layout_call(&host, &*store, sequence, request);
            eprintln!("SEQ_LAYOUT_ACCEPTED_{case}_CALL_{sequence}={receipt:?}");
            assert_eq!(receipt.status, "  ");
        }
        drop(host);
        drop(service);
        let before = seq_layout_rows(&*store);
        let reopened = ImsService::open(store.clone(), Default::default()).unwrap();
        assert_eq!(seq_layout_rows(&*store), before);
        let host = seq_layout_make_host(reopened);
        let root = seq_layout_call(
            &host,
            &*store,
            4,
            seq_layout_request(ImsOperation::GetUnique, 4, &[], Some(b"A1")),
        );
        assert_eq!(root.status, "  ");
        assert_eq!(
            root.segments[0].data,
            if case == "hsam-root-variable" {
                b"A1XY".as_slice()
            } else {
                b"A1X".as_slice()
            }
        );
        let child = seq_layout_call(
            &host,
            &*store,
            5,
            seq_layout_request(ImsOperation::GetNext, 5, &[], None),
        );
        assert_eq!(child.status, "  ");
        assert_eq!(
            child.segments[0].data,
            if case == "hsam-dependent-variable" {
                b"C1XY".as_slice()
            } else {
                b"C1Y".as_slice()
            }
        );
        seq_layout_log_rows("ACCEPTED_RETAINED", &*store);
        eprintln!("SEQ_LAYOUT_ACCEPTED_REOPEN_{case}_PASS");
    }
    assert_eq!(
        actual,
        Err(HostProblem::Malformed),
        "normative rejected layout: {case}"
    );
    assert_eq!(
        after, before,
        "rejected layout must preserve every provider payload/version"
    );
}
fn seq_layout_assert_replay_rows(before: &[ProviderStateRecord], after: &[ProviderStateRecord]) {
    let is_audit =
        |row: &&ProviderStateRecord| matches!(row.namespace.as_str(), "durable-audit-v1");
    assert_eq!(
        before
            .iter()
            .filter(|row| !is_audit(row))
            .collect::<Vec<_>>(),
        after
            .iter()
            .filter(|row| !is_audit(row))
            .collect::<Vec<_>>(),
        "replay preserves all non-audit provider payload/version rows"
    );
    eprintln!(
        "SEQ_LAYOUT_REPLAY_AUDIT_BEFORE_AFTER={:?}/{:?}",
        before
            .iter()
            .filter(is_audit)
            .map(|r| (&r.namespace, &r.key, r.version))
            .collect::<Vec<_>>(),
        after
            .iter()
            .filter(is_audit)
            .map(|r| (&r.namespace, &r.key, r.version))
            .collect::<Vec<_>>()
    );
}

fn seq_layout_programming(store: Arc<dyn ProviderStateStore>, case: &str, path: Option<PathBuf>) {
    let service = ImsService::open(store.clone(), Default::default()).unwrap();
    let metadata = seq_layout_catalog(case);
    let identity = service.install_metadata(metadata.clone()).unwrap();
    let host = seq_layout_make_host(service.clone());
    assert_eq!(
        seq_layout_call(
            &host,
            &*store,
            1,
            seq_layout_request(ImsOperation::Schedule, 1, &[], None)
        )
        .status,
        "  "
    );
    let sequential = case.starts_with("hsam") || case.starts_with("shsam");
    let first = if sequential {
        seq_layout_load(case, 2)
    } else {
        seq_layout_request(ImsOperation::Insert, 2, b"A1X", None)
    };
    let receipt = seq_layout_call(&host, &*store, 2, first.clone());
    assert_eq!(receipt.status, "  ");
    assert_eq!(
        receipt.affected_segments,
        if case == "hsam-fixed" {
            3
        } else if sequential {
            2
        } else {
            1
        }
    );
    let ranged = case == "hisam-ranged" || case == "hidam-ranged";
    if !sequential {
        let second = seq_layout_call(
            &host,
            &*store,
            3,
            seq_layout_request(
                ImsOperation::Insert,
                3,
                if ranged { b"B2YZ" } else { b"B2Y" },
                None,
            ),
        );
        assert_eq!(second.status, "  ");
        assert_eq!(second.affected_segments, 1);
    }
    assert_eq!(
        seq_layout_call(
            &host,
            &*store,
            4,
            seq_layout_request(ImsOperation::Commit, 4, &[], None)
        )
        .status,
        "  "
    );
    for (sequence, operation) in [
        (5, ImsOperation::GetUnique),
        (6, ImsOperation::GetHoldUnique),
    ] {
        let got = seq_layout_call(
            &host,
            &*store,
            sequence,
            seq_layout_request(operation, sequence, &[], Some(b"A1")),
        );
        assert_eq!(got.status, "  ");
        assert_eq!(got.segments[0].data, b"A1X");
    }
    let before = seq_layout_rows(&*store);
    assert_eq!(seq_layout_call(&host, &*store, 2, first.clone()), receipt);
    seq_layout_assert_replay_rows(&before, &seq_layout_rows(&*store));
    let expected = if case == "hsam-fixed" {
        b"C1Y".as_slice()
    } else if ranged {
        b"B2YZ".as_slice()
    } else {
        b"B2Y".as_slice()
    };
    assert_eq!(
        seq_layout_call(
            &host,
            &*store,
            7,
            seq_layout_request(ImsOperation::GetNext, 7, &[], None)
        )
        .segments[0]
            .data,
        expected
    );
    let before = seq_layout_rows(&*store);
    drop(host);
    drop(service);
    let reopened_store: Arc<dyn ProviderStateStore> = if let Some(path) = path {
        Arc::new(
            SqliteStateStore::open(
                &format!("sqlite:{}?mode=rw", path.display()),
                64 * 1024 * 1024,
                262_144,
            )
            .unwrap(),
        )
    } else if let Ok(url) = std::env::var("SEQ_LAYOUT_POSTGRES_URL") {
        Arc::new(
            mainframe_env_store::PostgresStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap(),
        )
    } else {
        store.clone()
    };
    let reopened = ImsService::open(reopened_store.clone(), Default::default()).unwrap();
    assert_eq!(seq_layout_rows(&*reopened_store), before);
    assert_eq!(reopened.install_metadata(metadata).unwrap(), identity);
    let host = seq_layout_make_host(reopened);
    let before = seq_layout_rows(&*reopened_store);
    assert_eq!(seq_layout_call(&host, &*reopened_store, 2, first), receipt);
    seq_layout_assert_replay_rows(&before, &seq_layout_rows(&*reopened_store));
    assert_eq!(
        seq_layout_call(
            &host,
            &*reopened_store,
            8,
            seq_layout_request(ImsOperation::GetUnique, 8, &[], Some(b"A1"))
        )
        .segments[0]
            .data,
        b"A1X"
    );
    assert_eq!(
        seq_layout_call(
            &host,
            &*reopened_store,
            9,
            seq_layout_request(ImsOperation::GetNext, 9, &[], None)
        )
        .segments[0]
            .data,
        expected
    );
    eprintln!("SEQ_LAYOUT_PUBLIC_CONTROL_{case}_PASS");
}

#[test]
#[ignore = "requires exact external old/new public SQLite bytes"]
fn seq_layout_reader_public_pure() {
    let path = PathBuf::from(std::env::var_os("SEQ_LAYOUT_READER_DB").unwrap());
    let bytes = std::fs::read(&path).unwrap();
    let store: Arc<dyn ProviderStateStore> = Arc::new(
        SqliteStateStore::open(
            &format!("sqlite:{}?mode=rw", path.display()),
            64 * 1024 * 1024,
            262_144,
        )
        .unwrap(),
    );
    let before = seq_layout_rows(&*store);
    let result = ImsService::open(store.clone(), Default::default());
    if std::env::var("SEQ_LAYOUT_READER_EXPECT").unwrap() == "invalid" {
        assert!(matches!(result, Err(HostProblem::InfrastructureFailure)));
    } else {
        result.unwrap();
        let row = store
            .get_provider_state("ims-v1-generic-database", "FIXDB")
            .unwrap()
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        let engine = mainframe_env_ims::database::DatabaseEngine::restore(
            serde_json::from_value(value["value"].clone()).unwrap(),
            Default::default(),
        )
        .unwrap();
        assert_eq!(
            engine.export_records()[0].data,
            if std::env::var("SEQ_LAYOUT_READER_CASE").unwrap() == "hsam-root-variable" {
                b"A1XY".as_slice()
            } else {
                b"A1X".as_slice()
            }
        );
    }
    assert_eq!(seq_layout_rows(&*store), before);
    drop(store);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    eprintln!("SEQ_LAYOUT_PUBLIC_PURE_READER_PASS");
}

#[test]
#[ignore = "requires isolated configured PostgreSQL task database and explicit case"]
fn seq_layout_postgres_public_case() {
    let case = std::env::var("SEQ_LAYOUT_POSTGRES_CASE").unwrap();
    assert!(
        [
            "shisam-multiple",
            "shsam-multiple",
            "hsam-root-variable",
            "hsam-dependent-variable",
            "shsam-variable",
            "shisam-variable",
            "hsam-fixed",
            "shsam-fixed",
            "shisam-fixed",
            "hisam-ranged",
            "hidam-ranged"
        ]
        .contains(&case.as_str())
    );
    let url = std::env::var("SEQ_LAYOUT_POSTGRES_URL").unwrap();
    let store = Arc::new(
        mainframe_env_store::PostgresStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap(),
    );
    if case.ends_with("fixed") || case.ends_with("ranged") {
        seq_layout_programming(store, &case, None);
    } else {
        seq_layout_reject(store, &case);
    }
    eprintln!("SEQ_LAYOUT_POSTGRES_PUBLIC_{case}_PASS");
}

#[test]
fn seq_layout_public_shisam_multiple_memory() {
    seq_layout_reject(
        Arc::new(MemoryStore::new(Default::default())),
        "shisam-multiple",
    );
}

#[test]
fn seq_layout_public_shisam_multiple_sqlite() {
    let (store, path) = seq_layout_sqlite("shisam-multiple");
    eprintln!("SEQ_LAYOUT_PUBLIC_ARTIFACT={}", path.display());
    seq_layout_reject(store, "shisam-multiple");
}

#[test]
fn seq_layout_public_shsam_multiple_memory() {
    seq_layout_reject(
        Arc::new(MemoryStore::new(Default::default())),
        "shsam-multiple",
    );
}

#[test]
fn seq_layout_public_shsam_multiple_sqlite() {
    let (store, path) = seq_layout_sqlite("shsam-multiple");
    eprintln!("SEQ_LAYOUT_PUBLIC_ARTIFACT={}", path.display());
    seq_layout_reject(store, "shsam-multiple");
}

#[test]
fn seq_layout_public_hsam_root_variable_memory() {
    seq_layout_reject(
        Arc::new(MemoryStore::new(Default::default())),
        "hsam-root-variable",
    );
}

#[test]
fn seq_layout_public_hsam_root_variable_sqlite() {
    let (store, path) = seq_layout_sqlite("hsam-root-variable");
    eprintln!("SEQ_LAYOUT_PUBLIC_ARTIFACT={}", path.display());
    seq_layout_reject(store, "hsam-root-variable");
}

#[test]
fn seq_layout_public_hsam_dependent_variable_memory() {
    seq_layout_reject(
        Arc::new(MemoryStore::new(Default::default())),
        "hsam-dependent-variable",
    );
}

#[test]
fn seq_layout_public_hsam_dependent_variable_sqlite() {
    let (store, path) = seq_layout_sqlite("hsam-dependent-variable");
    eprintln!("SEQ_LAYOUT_PUBLIC_ARTIFACT={}", path.display());
    seq_layout_reject(store, "hsam-dependent-variable");
}

#[test]
fn seq_layout_public_shsam_variable_memory() {
    seq_layout_reject(
        Arc::new(MemoryStore::new(Default::default())),
        "shsam-variable",
    );
}

#[test]
fn seq_layout_public_shsam_variable_sqlite() {
    let (store, path) = seq_layout_sqlite("shsam-variable");
    eprintln!("SEQ_LAYOUT_PUBLIC_ARTIFACT={}", path.display());
    seq_layout_reject(store, "shsam-variable");
}

#[test]
fn seq_layout_public_shisam_variable_control_memory() {
    seq_layout_reject(
        Arc::new(MemoryStore::new(Default::default())),
        "shisam-variable",
    );
}

#[test]
fn seq_layout_public_shisam_variable_control_sqlite() {
    let (store, path) = seq_layout_sqlite("shisam-variable");
    eprintln!("SEQ_LAYOUT_PUBLIC_ARTIFACT={}", path.display());
    seq_layout_reject(store, "shisam-variable");
}

#[test]
fn seq_layout_public_hsam_fixed_memory() {
    seq_layout_programming(
        Arc::new(MemoryStore::new(Default::default())),
        "hsam-fixed",
        None,
    );
}

#[test]
fn seq_layout_public_hsam_fixed_sqlite() {
    let (store, path) = seq_layout_sqlite("hsam-fixed");
    eprintln!("SEQ_LAYOUT_PUBLIC_CONTROL_ARTIFACT={}", path.display());
    seq_layout_programming(store, "hsam-fixed", Some(path));
}

#[test]
fn seq_layout_public_shsam_fixed_memory() {
    seq_layout_programming(
        Arc::new(MemoryStore::new(Default::default())),
        "shsam-fixed",
        None,
    );
}

#[test]
fn seq_layout_public_shsam_fixed_sqlite() {
    let (store, path) = seq_layout_sqlite("shsam-fixed");
    eprintln!("SEQ_LAYOUT_PUBLIC_CONTROL_ARTIFACT={}", path.display());
    seq_layout_programming(store, "shsam-fixed", Some(path));
}

#[test]
fn seq_layout_public_shisam_fixed_memory() {
    seq_layout_programming(
        Arc::new(MemoryStore::new(Default::default())),
        "shisam-fixed",
        None,
    );
}

#[test]
fn seq_layout_public_shisam_fixed_sqlite() {
    let (store, path) = seq_layout_sqlite("shisam-fixed");
    eprintln!("SEQ_LAYOUT_PUBLIC_CONTROL_ARTIFACT={}", path.display());
    seq_layout_programming(store, "shisam-fixed", Some(path));
}

#[test]
fn seq_layout_public_hisam_ranged_memory() {
    seq_layout_programming(
        Arc::new(MemoryStore::new(Default::default())),
        "hisam-ranged",
        None,
    );
}

#[test]
fn seq_layout_public_hisam_ranged_sqlite() {
    let (store, path) = seq_layout_sqlite("hisam-ranged");
    eprintln!("SEQ_LAYOUT_PUBLIC_CONTROL_ARTIFACT={}", path.display());
    seq_layout_programming(store, "hisam-ranged", Some(path));
}

#[test]
fn seq_layout_public_hidam_ranged_memory() {
    seq_layout_programming(
        Arc::new(MemoryStore::new(Default::default())),
        "hidam-ranged",
        None,
    );
}

#[test]
fn seq_layout_public_hidam_ranged_sqlite() {
    let (store, path) = seq_layout_sqlite("hidam-ranged");
    eprintln!("SEQ_LAYOUT_PUBLIC_CONTROL_ARTIFACT={}", path.display());
    seq_layout_programming(store, "hidam-ranged", Some(path));
}
