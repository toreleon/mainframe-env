//! Signed-selected independent literals through the durable coordinator.
use super::application_recovery::{recovery_db, run_recovery_machine};
use super::*;
use mainframe_env_host_api::{
    ImsExecutionContext, ImsNavigationRequest, ImsPcbFeedbackRequestV1, ImsPcbKeyFeedbackV1,
};
use mainframe_env_ims::{ImsGenericLoadImage, ImsGenericLoadRecord};

#[path = "primary_position_tests/retained.rs"]
mod retained;

const APP: &str = "SIGNED-IMS-APPLICATION";
const PREFIX: &[&[u8]] = &[b"A       (AKEY    EQA1)", b"B       (BKEY    EQB11)"];
const DATA_MISS: &[&[u8]] = &[
    b"A       (AKEY    EQA1)",
    b"B       (BKEY    GEB11*BDATA   EQ14)",
    b"C       (CKEY    EQC113)",
];
const EXACT_MISS: &[&[u8]] = &[
    b"A       (AKEY    EQA1)",
    b"B       (BKEY    EQB11)",
    b"C       (CKEY    EQC113)",
];
const UU: &[&[u8]] = &[b"A       *U ", b"B       *U ", b"C        "];
const V: &[&[u8]] = &[b"A        ", b"B       *V ", b"C        "];

fn literal_catalog() -> ImsMetadataCatalog {
    let field = |name: &str, offset, length, sequence| ImsFieldMetadata {
        name: Some(name.into()),
        offset,
        length,
        sequence,
        unique: sequence,
    };
    let segment = |name: &str, parent: Option<&str>, length, fields| ImsSegmentMetadata {
        name: name.into(),
        parent: parent.map(str::to_owned),
        min_length: length,
        max_length: length,
        fields,
    };
    ImsMetadataCatalog {
        schema_version: IMS_METADATA_SCHEMA_V1.into(),
        databases: vec![ImsDatabaseMetadata {
            name: "AUTHDB".into(),
            version: 1,
            organization: ImsDatabaseOrganization::Hidam,
            gsam_format: None,
            segments: vec![
                segment("A", None, 3, vec![field("AKEY", 0, 2, true)]),
                segment(
                    "B",
                    Some("A"),
                    6,
                    vec![field("BKEY", 0, 3, true), field("BDATA", 3, 2, false)],
                ),
                segment("C", Some("B"), 5, vec![field("CKEY", 0, 4, true)]),
                segment("D", Some("B"), 5, vec![field("DKEY", 0, 4, true)]),
                segment("E", Some("A"), 4, vec![field("EKEY", 0, 3, true)]),
            ],
            secondary_indexes: vec![],
            logical_relationships: vec![],
        }],
        psbs: vec![ImsPsbMetadata {
            name: "AUTHPSB".into(),
            database_level: ImsDbLevel::Current,
            pcbs: vec![ImsPcbMetadata::Database(ImsDatabasePcbMetadata {
                name: "POSITION".into(),
                database: "AUTHDB".into(),
                database_version: Some(1),
                secondary_index: None,
                processing_options: "AP".into(),
                sensitive_segments: [
                    ("A", None),
                    ("B", Some("A")),
                    ("C", Some("B")),
                    ("D", Some("B")),
                    ("E", Some("A")),
                ]
                .into_iter()
                .map(|(name, parent)| ImsSensitiveSegmentMetadata {
                    name: name.into(),
                    parent: parent.map(str::to_owned),
                    processing_options: None,
                })
                .collect(),
            })],
        }],
    }
}

fn literal_image() -> ImsGenericLoadImage {
    let record = |segment: &str, parent, data: &[u8]| ImsGenericLoadRecord {
        segment: segment.into(),
        parent,
        data: data.to_vec(),
    };
    ImsGenericLoadImage {
        database: "AUTHDB".into(),
        records: vec![
            record("A", None, b"A1r"),
            record("B", Some(0), b"B1114x"),
            record("C", Some(1), b"C111a"),
            record("C", Some(1), b"C112b"),
            record("D", Some(1), b"D111e"),
            record("B", Some(0), b"B1215x"),
            record("C", Some(5), b"C121c"),
            record("B", Some(0), b"B1316x"),
            record("E", Some(0), b"E11f"),
            record("A", None, b"A2r"),
            record("B", Some(9), b"B2117x"),
            record("C", Some(10), b"C211d"),
        ],
    }
}

fn batch(server: &ProductServer, run: &str, execution: &str) -> Invocation {
    let mut invocation = tm_invocation(run, execution);
    invocation.service_class = ServiceClass::Batch;
    invocation.deadline_tick = server.jes_clock.now_tick().unwrap() + 60_000;
    invocation.principal = Principal::new(
        PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap(),
        [CapabilityId::new("host.ims.write", InvocationLimits::default()).unwrap()]
            .into_iter()
            .collect(),
        InvocationLimits::default(),
    )
    .unwrap();
    invocation
}

fn call(seq: u64, operation: ImsOperation, ssas: &[&[u8]]) -> HostRequest {
    HostRequest::ImsNavigation(ImsNavigationRequest {
        request: recovery_db(operation, seq, "primary-proof"),
        context: ImsExecutionContext::DbBatch,
        ssas: ssas.iter().map(|s| s.to_vec()).collect(),
    })
}

fn drive(
    server: &ProductServer,
    store: &Arc<dyn PlatformStore>,
    run: &str,
    execution: &str,
    request: HostRequest,
) -> EffectResult {
    let invocation = batch(server, run, execution);
    let key = request.mutation().unwrap().idempotency_key.clone();
    let digest = mainframe_env_host_api::canonical_request_digest(&request).unwrap();
    let mut results = run_recovery_machine(server, store.clone(), &invocation, vec![request]);
    assert_eq!(
        results.len(),
        1,
        "setup coordinator selected exactly one effect"
    );
    let result = results.remove(0);
    let journal = store
        .effect(&key)
        .unwrap()
        .expect("setup real coordinator journal");
    eprintln!(
        "COORDINATOR {execution}: {:?}, journal {:?}",
        result.outcome, journal.state
    );
    assert_eq!(
        journal.state,
        if result.outcome.is_ok() {
            EffectState::Completed
        } else {
            EffectState::Failed
        }
    );
    assert_eq!(journal.request_digest, digest);
    assert_eq!(
        journal.result_digest,
        Some(mainframe_env_host_api::canonical_result_digest(&result.outcome).unwrap())
    );
    result
}

fn ims(effect: EffectResult) -> Result<mainframe_env_host_api::ImsResult, HostProblem> {
    match effect.outcome? {
        HostResult::Ims(result) => Ok(result),
        other => panic!("setup unexpected selected result {other:?}"),
    }
}

struct SqliteFile(Option<std::path::PathBuf>);
impl Drop for SqliteFile {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            for suffix in ["", "-wal", "-shm"] {
                let mut name = path.as_os_str().to_os_string();
                name.push(suffix);
                let _ = std::fs::remove_file(std::path::PathBuf::from(name));
            }
        }
    }
}

fn exercise(sqlite: bool, case: &str) {
    let run = format!("signed-primary-{case}");
    let file = sqlite.then(|| {
        std::env::temp_dir().join(format!(
            "signed-primary-failfirst-{}-{case}.sqlite",
            std::process::id()
        ))
    });
    let _cleanup = SqliteFile(file.clone());
    let mut config = config();
    let store: Arc<dyn PlatformStore> = if let Some(file) = &file {
        let url = format!("sqlite:{}?mode=rwc", file.display());
        config.store_profile = crate::StoreProfile::Sqlite;
        config.sqlite_url = url.clone();
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap())
    } else {
        Arc::new(MemoryStore::new(Default::default()))
    };
    let trust = Arc::new(test_package_trust());
    let server = ProductServer::open_with_package_trust(
        config,
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust.clone(),
    )
    .unwrap();
    let mut package = signed_ims_package(&trust, 1, 1);
    package.sections.ims_metadata = Some(literal_catalog());
    resign_package(&mut package, &trust);
    let staged = server
        .install_application_package_v2(&package)
        .expect("setup signed package");
    server.publish_application_generation(&staged).unwrap();
    server
        .bootstrap_administrator("IBMUSER", b"TESTPASS")
        .unwrap();
    for (class, name) in [("IMSPSB", "AUTHPSB"), ("IMSDB", "AUTHDB")] {
        server
            .racf
            .define_profile(class, name, "IBMUSER", Some(AccessIntent::Control))
            .unwrap();
    }
    let install = batch(&server, &run, "primary-install");
    let mut load = recovery_db(ImsOperation::Load, 2, "primary-proof");
    load.data = serde_json::to_vec(&literal_image()).unwrap();
    for req in [
        recovery_db(ImsOperation::Schedule, 1, "primary-proof"),
        load,
        recovery_db(ImsOperation::Commit, 3, "primary-proof"),
    ] {
        assert_eq!(
            server
                .ims_execute_selected(APP, &install, &req)
                .expect("setup signed load/schedule/commit")
                .status,
            "  "
        );
    }
    // Each completed coordinator execution has a distinct real identity.
    let prefix = ims(drive(
        &server,
        &store,
        &run,
        "primary-prefix",
        call(4, ImsOperation::GetUnique, PREFIX),
    ))
    .expect("setup actual prefix GU");
    assert_eq!(prefix.status, "  ");
    assert_eq!(prefix.segments.len(), 1);
    assert_eq!(prefix.segments[0].data, b"B1114x");
    assert_eq!(
        prefix.segments[0].parent_key.as_deref(),
        Some(b"A1".as_slice())
    );
    eprintln!(
        "SETUP-PASS signed/{run}: published {} real prefix GU => B1114x",
        staged.identity
    );
    if case == "setup" {
        let found = ims(drive(
            &server,
            &store,
            &run,
            "primary-setup-qualified",
            call(
                5,
                ImsOperation::GetUnique,
                &[
                    b"A       (AKEY    EQA1)",
                    b"B       (BKEY    GEB11*BDATA   EQ14)",
                    b"C       (CKEY    EQC111)",
                ],
            ),
        ))
        .unwrap();
        assert_eq!(found.status, "  ");
        assert_eq!(found.segments[0].data, b"C111a");
        let next = ims(drive(
            &server,
            &store,
            &run,
            "primary-setup-next",
            call(6, ImsOperation::GetNext, &[]),
        ))
        .unwrap();
        assert_eq!(next.segments[0].data, b"C112b");
        eprintln!("SETUP-PASS signed/{run}: literal qualified C111a then ordinary C112b");
        return;
    }
    if case == "feedback" {
        let request = HostRequest::ImsPcbFeedbackV1(ImsPcbFeedbackRequestV1 {
            request: recovery_db(ImsOperation::GetNext, 5, "primary-proof"),
            context: ImsExecutionContext::DbBatch,
            ssas: Some(DATA_MISS.iter().map(|s| s.to_vec()).collect()),
            key_capacity: 5,
        });
        let effect = drive(&server, &store, &run, "primary-search", request);
        let HostResult::ImsPcbFeedbackV1(actual) = effect.outcome.unwrap() else {
            panic!("setup selected feedback variant")
        };
        assert_eq!(actual.result.status, "GE");
        assert!(actual.result.segments.is_empty());
        assert_eq!(actual.feedback.transferred_data_length, 0);
        eprintln!(
            "WITNESS signed/{run}: actual {:?}, length {:?}; expected B/2/A1B11/5",
            actual.feedback.key,
            actual.feedback.key.valid_length()
        );
        assert_eq!(
            actual.feedback.key,
            ImsPcbKeyFeedbackV1::Valid {
                segment_name: "B".into(),
                segment_level: 2,
                bytes: b"A1B11".to_vec(),
            }
        );
        assert_eq!(actual.feedback.key.valid_length(), Some(5));
        return;
    }
    let miss = ims(drive(
        &server,
        &store,
        &run,
        "primary-search",
        call(
            5,
            ImsOperation::GetNext,
            if case == "exact" {
                EXACT_MISS
            } else {
                DATA_MISS
            },
        ),
    ))
    .expect("setup source actual qualified GN");
    assert_eq!(miss.status, "GE");
    assert!(miss.segments.is_empty());
    eprintln!("SETUP-PASS signed/{run}: actual qualified GN => GE/no data");
    let operands = match case {
        "uu" => UU,
        "v" => V,
        _ => &[],
    };
    let actual = ims(drive(
        &server,
        &store,
        &run,
        "primary-continuation",
        call(6, ImsOperation::GetNext, operands),
    ));
    eprintln!("WITNESS signed/{run}: {actual:?}");
    let expected: &[u8] = match case {
        "ordinary" => b"E11f",
        "exact" => b"D111e",
        _ => b"C111a",
    };
    assert_eq!(
        actual.map(|r| (
            r.status,
            r.segments.into_iter().map(|s| s.data).collect::<Vec<_>>()
        )),
        Ok(("  ".into(), vec![expected.to_vec()])),
        "literal signed {case} continuation"
    );
}

macro_rules! cases {
    ($sqlite:expr, $($name:ident => $case:literal),+ $(,)?) => {$ (
        #[test]
        fn $name() { exercise($sqlite, $case); }
    )+};
}
cases!(false,
    ssa_primary_position_failfirst_signed_setup_memory => "setup",
    ssa_primary_position_failfirst_signed_feedback_memory => "feedback",
    ssa_primary_position_failfirst_signed_ordinary_memory => "ordinary",
    ssa_primary_position_failfirst_signed_uu_memory => "uu",
    ssa_primary_position_failfirst_signed_v_memory => "v",
    ssa_primary_position_failfirst_signed_exact_memory => "exact",
);
cases!(true,
    ssa_primary_position_failfirst_signed_setup_sqlite => "setup",
    ssa_primary_position_failfirst_signed_feedback_sqlite => "feedback",
    ssa_primary_position_failfirst_signed_ordinary_sqlite => "ordinary",
    ssa_primary_position_failfirst_signed_uu_sqlite => "uu",
    ssa_primary_position_failfirst_signed_v_sqlite => "v",
    ssa_primary_position_failfirst_signed_exact_sqlite => "exact",
);
