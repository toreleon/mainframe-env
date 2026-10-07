use super::*;
#[path = "ims_secondary_ssa_tests/mixed_boolean_tests.rs"]
mod mixed_boolean_tests;
#[path = "ims_secondary_ssa_tests/mixed_evaluation_tests.rs"]
mod mixed_evaluation_tests;
static NEXT_SECONDARY_FILE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
use mainframe_env_host_api::{
    ImsExecutionContext, ImsNavigationRequest, ImsSecondaryIndexMetadata,
};

fn exercise(server: &ProductServer, trust: &HmacSha256PackageTrust) {
    let mut package = signed_carddemo_package(trust, 1);
    let metadata = package.sections.ims_metadata.as_mut().unwrap();
    metadata.databases[0].segments[0]
        .fields
        .push(ImsFieldMetadata {
            name: Some("KIND".into()),
            offset: 6,
            length: 1,
            sequence: false,
            unique: false,
        });
    metadata.databases[0]
        .secondary_indexes
        .push(ImsSecondaryIndexMetadata {
            name: "BYVALUE".into(),
            source_segment: "PAUTSUM0".into(),
            target_segment: "PAUTSUM0".into(),
            source_fields: vec!["KIND".into()],
        });
    let ImsPcbMetadata::Database(mut pcb) = metadata.psbs[0].pcbs[0].clone() else {
        unreachable!()
    };
    pcb.name = "INDEXPCB".into();
    pcb.secondary_index = Some("BYVALUE".into());
    metadata.psbs[0].pcbs.push(ImsPcbMetadata::Database(pcb));
    resign_package(&mut package, trust);
    let staged = server.install_application_package_v2(&package).unwrap();
    server.publish_application_generation(&staged).unwrap();
    server
        .bootstrap_administrator("IBMUSER", b"TESTPASS")
        .unwrap();
    for (class, name) in [("IMSPSB", "PSBPAUTB"), ("IMSDB", "DBPAUTP0")] {
        server
            .racf
            .define_profile(class, name, "IBMUSER", Some(AccessIntent::Control))
            .unwrap();
    }
    let run = "signed-index";
    server
        .ims_execute_selected(
            "CARDDEMO-IMS",
            &tm_invocation(run, "schedule"),
            &carddemo_request(ImsOperation::Schedule, 1, vec![]),
        )
        .unwrap();
    for (seq, key, kind) in [(2, b"000001", b'Z'), (3, b"000002", b'A')] {
        let mut data = vec![0xff; 100];
        data[..6].copy_from_slice(key);
        data[6] = kind;
        server
            .ims_execute_selected(
                "CARDDEMO-IMS",
                &tm_invocation(run, "insert"),
                &carddemo_request(ImsOperation::Insert, seq, data),
            )
            .unwrap();
    }
    let mut request = carddemo_request(ImsOperation::GetHoldNext, 4, vec![]);
    request.pcb = 3;
    request.segments.clear();
    let nav = ImsNavigationRequest {
        request,
        context: ImsExecutionContext::DbBatch,
        ssas: vec![b"PAUTSUM0(BYVALUE GEA)".to_vec()],
    };
    let found = server
        .ims_navigation_selected("CARDDEMO-IMS", &tm_invocation(run, "index"), &nav)
        .unwrap();
    assert_eq!(found.status, "  ");
    assert_eq!(&found.segments[0].data[..7], b"000002A");
    assert_eq!(
        server
            .ims_navigation_selected("CARDDEMO-IMS", &tm_invocation(run, "index"), &nav)
            .unwrap(),
        found
    );
    let mut replace = carddemo_request(ImsOperation::Replace, 5, found.segments[0].data.clone());
    replace.pcb = 3;
    replace.segments.clear();
    replace.data[99] = 0x80;
    assert_eq!(
        server
            .ims_execute_selected("CARDDEMO-IMS", &tm_invocation(run, "replace"), &replace)
            .unwrap()
            .status,
        "  "
    );
    let primary = server
        .ims_execute_selected(
            "CARDDEMO-IMS",
            &tm_invocation(run, "primary"),
            &carddemo_request(ImsOperation::GetUnique, 6, vec![]),
        )
        .unwrap();
    assert_eq!(&primary.segments[0].data[..7], b"000001Z");
}

#[test]
fn secondary_ssa_signed_package_memory() {
    let trust = Arc::new(test_package_trust());
    let server = ProductServer::memory_with_package_trust(config(), trust.clone()).unwrap();
    exercise(&server, &trust);
}

#[test]
fn secondary_ssa_signed_package_sqlite_fresh_reopen() {
    let file = std::env::temp_dir().join(format!(
        "ims-signed-secondary-{}-{}.sqlite",
        std::process::id(),
        NEXT_SECONDARY_FILE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    let mut settings = config();
    settings.store_profile = crate::StoreProfile::Sqlite;
    settings.sqlite_url = url.clone();
    let trust = Arc::new(test_package_trust());
    let open = || {
        ProductServer::open_with_package_trust(
            settings.clone(),
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap()),
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
            trust.clone(),
        )
        .unwrap()
    };
    let server = open();
    exercise(&server, &trust);
    drop(server);
    let server = open();
    let mut request = carddemo_request(ImsOperation::GetHoldNext, 4, vec![]);
    request.pcb = 3;
    let nav = ImsNavigationRequest {
        request,
        context: ImsExecutionContext::DbBatch,
        ssas: vec![b"PAUTSUM0(BYVALUE GEA)".to_vec()],
    };
    let replay = server
        .ims_navigation_selected(
            "CARDDEMO-IMS",
            &tm_invocation("signed-index", "index"),
            &nav,
        )
        .unwrap();
    assert_eq!(&replay.segments[0].data[..7], b"000002A");
    assert_eq!(replay.segments[0].data[99], 0xff);
    let mut next = nav;
    next.request = carddemo_request(ImsOperation::GetNext, 7, vec![]);
    next.request.pcb = 3;
    next.ssas = vec![b"PAUTSUM0 ".to_vec()];
    assert_eq!(
        &server
            .ims_navigation_selected(
                "CARDDEMO-IMS",
                &tm_invocation("signed-index", "next"),
                &next
            )
            .unwrap()
            .segments[0]
            .data[..7],
        b"000001Z"
    );
    drop(server);
    std::fs::remove_file(file).unwrap();
}

fn composite(server: &ProductServer, trust: &HmacSha256PackageTrust) {
    let mut package = signed_carddemo_package(trust, 1);
    let metadata = package.sections.ims_metadata.as_mut().unwrap();
    for (name, offset) in [("ZONE", 8), ("KIND", 10)] {
        metadata.databases[0].segments[1]
            .fields
            .push(ImsFieldMetadata {
                name: Some(name.into()),
                offset,
                length: 1,
                sequence: false,
                unique: false,
            });
    }
    metadata.databases[0]
        .secondary_indexes
        .push(ImsSecondaryIndexMetadata {
            name: "BYVALUE".into(),
            source_segment: "PAUTDTL1".into(),
            target_segment: "PAUTSUM0".into(),
            source_fields: vec!["ZONE".into(), "KIND".into()],
        });
    let ImsPcbMetadata::Database(mut pcb) = metadata.psbs[0].pcbs[0].clone() else {
        unreachable!()
    };
    pcb.name = "COMPOS".into();
    pcb.secondary_index = Some("BYVALUE".into());
    metadata.psbs[0].pcbs.push(ImsPcbMetadata::Database(pcb));
    resign_package(&mut package, trust);
    let staged = server.install_application_package_v2(&package).unwrap();
    server.publish_application_generation(&staged).unwrap();
    server
        .bootstrap_administrator("IBMUSER", b"TESTPASS")
        .unwrap();
    for (class, name) in [("IMSPSB", "PSBPAUTB"), ("IMSDB", "DBPAUTP0")] {
        server
            .racf
            .define_profile(class, name, "IBMUSER", Some(AccessIntent::Control))
            .unwrap();
    }
    let run = "signed-composite";
    server
        .ims_execute_selected(
            "CARDDEMO-IMS",
            &tm_invocation(run, "schedule"),
            &carddemo_request(ImsOperation::Schedule, 1, vec![]),
        )
        .unwrap();
    let mut records = vec![];
    for (i, key, zone, kind) in [(0, b"000001", 0xff, 0xff), (2, b"000002", 0, 0xff)] {
        let mut root = vec![0x80; 100];
        root[..6].copy_from_slice(key);
        let mut child = vec![0; 200];
        child[..8].copy_from_slice(if i == 0 { b"CHILD001" } else { b"CHILD002" });
        child[8] = zone;
        child[10] = kind;
        records.push(mainframe_env_ims::ImsGenericLoadRecord {
            segment: "PAUTSUM0".into(),
            parent: None,
            data: root,
        });
        records.push(mainframe_env_ims::ImsGenericLoadRecord {
            segment: "PAUTDTL1".into(),
            parent: Some(i),
            data: child,
        });
    }
    let image = mainframe_env_ims::ImsGenericLoadImage {
        database: "DBPAUTP0".into(),
        records,
    };
    server
        .ims_execute_selected(
            "CARDDEMO-IMS",
            &tm_invocation(run, "load"),
            &carddemo_request(ImsOperation::Load, 2, serde_json::to_vec(&image).unwrap()),
        )
        .unwrap();
    let mut request = carddemo_request(ImsOperation::GetHoldUnique, 3, vec![]);
    request.pcb = 3;
    let mut raw = b"PAUTSUM0(BYVALUE EQ".to_vec();
    raw.extend([0, 255, b')']);
    let nav = ImsNavigationRequest {
        request,
        context: ImsExecutionContext::DbBatch,
        ssas: vec![raw],
    };
    let result = server
        .ims_navigation_selected("CARDDEMO-IMS", &tm_invocation(run, "indexed"), &nav)
        .unwrap();
    assert_eq!(result.status, "  ");
    assert_eq!(&result.segments[0].data[..6], b"000002");
    assert_eq!(
        server
            .ims_navigation_selected("CARDDEMO-IMS", &tm_invocation(run, "indexed"), &nav)
            .unwrap(),
        result
    );
    let mut child = nav.clone();
    child.request = carddemo_request(ImsOperation::GetNextParent, 4, vec![]);
    child.request.pcb = 3;
    child.ssas = vec![b"PAUTDTL1 ".to_vec()];
    let found = server
        .ims_navigation_selected("CARDDEMO-IMS", &tm_invocation(run, "child"), &child)
        .unwrap();
    assert_eq!(&found.segments[0].data[..8], b"CHILD002");
    assert_eq!(found.segments[0].data[8], 0);
    assert_eq!(found.segments[0].data[10], 255);
    let primary = server
        .ims_execute_selected(
            "CARDDEMO-IMS",
            &tm_invocation(run, "primary"),
            &carddemo_request(ImsOperation::GetUnique, 5, vec![]),
        )
        .unwrap();
    assert_eq!(&primary.segments[0].data[..6], b"000001");
}

#[test]
fn secondary_ssa_signed_package_binary_composite_distinct_source_target_memory() {
    let trust = Arc::new(test_package_trust());
    let server = ProductServer::memory_with_package_trust(config(), trust.clone()).unwrap();
    composite(&server, &trust);
}

#[test]
fn secondary_ssa_signed_package_binary_composite_distinct_source_target_sqlite() {
    let file = std::env::temp_dir().join(format!(
        "ims-signed-composite-{}-{}.sqlite",
        std::process::id(),
        NEXT_SECONDARY_FILE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    let mut settings = config();
    settings.store_profile = crate::StoreProfile::Sqlite;
    settings.sqlite_url = url.clone();
    let trust = Arc::new(test_package_trust());
    let server = ProductServer::open_with_package_trust(
        settings,
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap()),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust.clone(),
    )
    .unwrap();
    composite(&server, &trust);
    drop(server);
    std::fs::remove_file(file).unwrap();
}
