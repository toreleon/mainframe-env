//! Independent literal public-provider histories; no manufactured positions.
use super::*;
use mainframe_env_host_api::{ImsPcbFeedbackRequestV1, ImsPcbKeyFeedbackV1};

mod fences;
mod publication;
mod retained;
mod trace_tests;

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

fn metadata() -> ImsMetadataCatalog {
    let segment = |name: &str, parent: Option<&str>, length, key: &str, width| {
        let mut fields = vec![ImsFieldMetadata {
            name: Some(key.into()),
            offset: 0,
            length: width,
            sequence: true,
            unique: true,
        }];
        if name == "B" {
            fields.push(ImsFieldMetadata {
                name: Some("BDATA".into()),
                offset: 3,
                length: 2,
                sequence: false,
                unique: false,
            });
        }
        ImsSegmentMetadata {
            name: name.into(),
            parent: parent.map(str::to_owned),
            min_length: length,
            max_length: length,
            fields,
        }
    };
    ImsMetadataCatalog {
        schema_version: IMS_METADATA_SCHEMA_V1.into(),
        databases: vec![ImsDatabaseMetadata {
            gsam_format: None,
            name: "GENDB".into(),
            version: 1,
            organization: ImsDatabaseOrganization::Hidam,
            segments: vec![
                segment("A", None, 3, "AKEY", 2),
                segment("B", Some("A"), 6, "BKEY", 3),
                segment("C", Some("B"), 5, "CKEY", 4),
                segment("D", Some("B"), 5, "DKEY", 4),
                segment("E", Some("A"), 4, "EKEY", 3),
            ],
            secondary_indexes: vec![],
            logical_relationships: vec![],
        }],
        psbs: vec![ImsPsbMetadata {
            name: "GENPSB".into(),
            database_level: ImsDbLevel::Current,
            pcbs: vec![ImsPcbMetadata::Database(ImsDatabasePcbMetadata {
                name: "POSITION".into(),
                database: "GENDB".into(),
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

fn image() -> ImsGenericLoadImage {
    let record = |segment: &str, parent, data: &[u8]| ImsGenericLoadRecord {
        segment: segment.into(),
        parent,
        data: data.to_vec(),
    };
    ImsGenericLoadImage {
        database: "GENDB".into(),
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

fn invoke(
    service: &Arc<ImsService>,
    run: &str,
    req: HostRequest,
) -> Result<HostResult, HostProblem> {
    req.validate(HostLimits::default())?;
    let invocation = invocation_class(run, ServiceClass::Batch);
    let mutation = req.mutation().unwrap();
    ims_providers(service.clone(), InvocationLimits::default())
        .remove(1)
        .invoke(
            &invocation,
            EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence: mutation.sequence,
                deadline_tick: invocation.deadline_tick,
                idempotency_key: Some(mutation.idempotency_key.clone()),
                request: req,
            },
        )
        .outcome
}

fn nav(
    service: &Arc<ImsService>,
    run: &str,
    seq: u64,
    op: ImsOperation,
    ssas: &[&[u8]],
) -> Result<ImsResult, HostProblem> {
    match invoke(
        service,
        run,
        HostRequest::ImsNavigation(navigation(run, seq, op, ssas)),
    )? {
        HostResult::Ims(result) => Ok(result),
        other => panic!("setup unexpected navigation variant {other:?}"),
    }
}

fn seed(store: Arc<dyn ProviderStateStore>, run: &str) -> Arc<ImsService> {
    seed_catalog(store, run, metadata())
}

fn seed_catalog(
    store: Arc<dyn ProviderStateStore>,
    run: &str,
    catalog: ImsMetadataCatalog,
) -> Arc<ImsService> {
    let service = ImsService::open(store, Default::default()).unwrap();
    service.install_metadata(catalog).expect("setup metadata");
    let mut load = request(run, ImsOperation::Load, 2, &[], b"");
    load.data = serde_json::to_vec(&image()).unwrap();
    for req in [
        request(run, ImsOperation::Schedule, 1, &[], b""),
        load,
        request(run, ImsOperation::Commit, 3, &[], b""),
    ] {
        let HostResult::Ims(result) =
            invoke(&service, run, HostRequest::Ims(req)).expect("setup route")
        else {
            panic!("setup result variant")
        };
        assert_eq!(result.status, "  ", "setup load/schedule/commit");
    }
    let prefix = nav(&service, run, 4, ImsOperation::GetUnique, PREFIX).expect("setup prefix GU");
    assert_eq!(prefix.status, "  ", "setup prefix GU status");
    assert_eq!(prefix.segments.len(), 1, "setup prefix GU count");
    assert_eq!(prefix.segments[0].data, b"B1114x", "setup prefix GU data");
    assert_eq!(
        prefix.segments[0].parent_key.as_deref(),
        Some(b"A1".as_slice())
    );
    eprintln!("SETUP-PASS public/{run}: real Batch prefix GU A1/B11 => B1114x");
    service
}

fn exercise(sqlite: bool, case: &str) {
    let run = format!("primary-{case}");
    let file = sqlite.then(|| {
        std::env::temp_dir().join(format!(
            "ims-primary-failfirst-{}-{}.sqlite",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ))
    });
    let _cleanup = SqliteFile(file.clone());
    let store: Arc<dyn ProviderStateStore> = if let Some(file) = &file {
        Arc::new(
            SqliteStateStore::open(
                &format!("sqlite:{}?mode=rwc", file.display()),
                64 * 1024 * 1024,
                262_144,
            )
            .unwrap(),
        )
    } else {
        Arc::new(MemoryStore::new(Default::default()))
    };
    let service = seed(store, &run);
    if case == "setup" {
        let success = nav(
            &service,
            &run,
            5,
            ImsOperation::GetUnique,
            &[
                b"A       (AKEY    EQA1)",
                b"B       (BKEY    GEB11*BDATA   EQ14)",
                b"C       (CKEY    EQC111)",
            ],
        )
        .expect("setup qualified successful GU");
        assert_eq!(success.status, "  ");
        assert_eq!(success.segments[0].data, b"C111a");
        assert_eq!(
            nav(&service, &run, 6, ImsOperation::GetNext, &[])
                .unwrap()
                .segments[0]
                .data,
            b"C112b"
        );
        eprintln!("SETUP-PASS public/{run}: literal qualified C111a then ordinary C112b");
        return;
    }
    if case == "feedback" {
        let feedback = HostRequest::ImsPcbFeedbackV1(ImsPcbFeedbackRequestV1 {
            request: request(&run, ImsOperation::GetNext, 5, &[], b""),
            context: ImsExecutionContext::DbBatch,
            ssas: Some(DATA_MISS.iter().map(|s| s.to_vec()).collect()),
            key_capacity: 5,
        });
        let HostResult::ImsPcbFeedbackV1(actual) = invoke(&service, &run, feedback).unwrap() else {
            panic!("setup feedback result variant")
        };
        assert_eq!(actual.result.status, "GE");
        assert!(actual.result.segments.is_empty());
        assert_eq!(actual.feedback.transferred_data_length, 0);
        eprintln!(
            "WITNESS public/{run}: actual {:?}, length {:?}; expected B/2/A1B11/5",
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
    let miss = nav(
        &service,
        &run,
        5,
        ImsOperation::GetNext,
        if case == "exact" {
            EXACT_MISS
        } else {
            DATA_MISS
        },
    )
    .expect("setup actual qualified GN");
    assert_eq!(miss.status, "GE", "setup qualified GN status");
    assert!(miss.segments.is_empty(), "setup GE data");
    eprintln!("SETUP-PASS public/{run}: actual qualified GN => GE/no data");
    let ssas = match case {
        "uu" => UU,
        "v" => V,
        _ => &[],
    };
    let actual = nav(&service, &run, 6, ImsOperation::GetNext, ssas);
    eprintln!("WITNESS public/{run}: {actual:?}");
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
        "literal {case} continuation"
    );
}

macro_rules! cases {
    ($sqlite:expr, $($name:ident => $case:literal),+ $(,)?) => {$ (
        #[test]
        fn $name() { exercise($sqlite, $case); }
    )+};
}
cases!(false,
    ssa_primary_position_failfirst_public_setup_memory => "setup",
    ssa_primary_position_failfirst_public_feedback_memory => "feedback",
    ssa_primary_position_failfirst_public_ordinary_memory => "ordinary",
    ssa_primary_position_failfirst_public_uu_memory => "uu",
    ssa_primary_position_failfirst_public_v_memory => "v",
    ssa_primary_position_failfirst_public_exact_memory => "exact",
);
cases!(true,
    ssa_primary_position_failfirst_public_setup_sqlite => "setup",
    ssa_primary_position_failfirst_public_feedback_sqlite => "feedback",
    ssa_primary_position_failfirst_public_ordinary_sqlite => "ordinary",
    ssa_primary_position_failfirst_public_uu_sqlite => "uu",
    ssa_primary_position_failfirst_public_v_sqlite => "v",
    ssa_primary_position_failfirst_public_exact_sqlite => "exact",
);
