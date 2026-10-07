use super::application_recovery::{recovery_request, run_recovery_machine};
use super::*;
use mainframe_env_host_api::{
    ImsExecutionContext, ImsLogicalRelationshipMetadata, ImsPcbFeedbackRequestV1,
    ImsPcbKeyFeedbackV1,
};
use mainframe_env_host_api::{
    ImsGsamRequest, ImsRecoveryCall, ImsRecoveryResult, ImsRestartSelection,
    ImsSecondaryIndexMetadata,
};

fn logical_metadata() -> ImsMetadataCatalog {
    let mut m = metadata(1);
    for s in &mut m.databases[0].segments {
        s.min_length = 3;
        s.max_length = 3;
        s.fields[0].length = 2;
        s.fields.push(ImsFieldMetadata {
            name: Some("KIND".into()),
            offset: 2,
            length: 1,
            sequence: false,
            unique: false,
        });
    }
    let mut parent = m.databases[0].clone();
    parent.name = "PARENTDB".into();
    parent.segments[0].name = "DROOT".into();
    parent.segments[1].name = "LPARENT".into();
    parent.segments[1].parent = Some("DROOT".into());
    parent.segments[0].fields[0].name = Some("DROOTKEY".into());
    parent.segments[1].fields[0].name = Some("LPKEY".into());
    m.databases[0]
        .logical_relationships
        .push(ImsLogicalRelationshipMetadata {
            child_database: "AUTHDB".into(),
            child_segment: "CHILD".into(),
            parent_database: "PARENTDB".into(),
            parent_segment: "LPARENT".into(),
            paired: true,
        });
    m.databases.push(parent);
    let ImsPcbMetadata::Database(source) = &mut m.psbs[0].pcbs[0] else {
        unreachable!()
    };
    source.sensitive_segments[1].processing_options = None;
    let mut pcb = source.clone();
    pcb.name = "PARENTPC".into();
    pcb.database = "PARENTDB".into();
    pcb.sensitive_segments[0].name = "DROOT".into();
    pcb.sensitive_segments[1].name = "LPARENT".into();
    pcb.sensitive_segments[1].parent = Some("DROOT".into());
    m.psbs[0].pcbs.push(ImsPcbMetadata::Database(pcb));
    let ImsPcbMetadata::Database(mut independent) = m.psbs[0].pcbs[0].clone() else {
        unreachable!()
    };
    independent.name = "OTHERPCB".into();
    m.psbs[0]
        .pcbs
        .push(ImsPcbMetadata::Database(independent.clone()));
    independent.name = "KEYPCB".into();
    independent.sensitive_segments[1].processing_options = Some("K".into());
    m.psbs[0].pcbs.push(ImsPcbMetadata::Database(independent));
    m.databases[0]
        .secondary_indexes
        .push(ImsSecondaryIndexMetadata {
            name: "BYKIND".into(),
            source_segment: "CHILD".into(),
            target_segment: "ROOT".into(),
            source_fields: vec!["KIND".into()],
        });
    let ImsPcbMetadata::Database(mut index) = m.psbs[0].pcbs[0].clone() else {
        unreachable!()
    };
    index.name = "INDEXPCB".into();
    index.secondary_index = Some("BYKIND".into());
    m.psbs[0].pcbs.push(ImsPcbMetadata::Database(index));
    let mut gsam = m.databases[1].clone();
    gsam.name = "FILEDB".into();
    gsam.organization = ImsDatabaseOrganization::Gsam;
    gsam.segments.truncate(1);
    gsam.segments[0].fields.clear();
    m.databases.push(gsam);
    let mut input = ImsDatabasePcbMetadata {
        name: "INPUT".into(),
        database: "FILEDB".into(),
        database_version: Some(1),
        secondary_index: None,
        processing_options: "G".into(),
        sensitive_segments: vec![ImsSensitiveSegmentMetadata {
            name: "DROOT".into(),
            parent: None,
            processing_options: None,
        }],
    };
    m.psbs[0].pcbs.push(ImsPcbMetadata::Database(input.clone()));
    input.name = "OUTPUT".into();
    input.processing_options = "L".into();
    m.psbs[0].pcbs.push(ImsPcbMetadata::Database(input));
    m
}

fn navigation(
    op: ImsOperation,
    seq: u64,
    pcb: u16,
    segment: Option<&str>,
    data: &[u8],
) -> ImsRequest {
    ImsRequest {
        operation: op,
        psb: (op == ImsOperation::Schedule).then(|| "AUTHPSB".into()),
        pcb,
        segments: segment.into_iter().map(str::to_owned).collect(),
        data: data.into(),
        qualifiers: vec![],
        checkpoint_id: None,
        max_segments: 8,
        mutation: Some(Mutation {
            sequence: seq,
            idempotency_key: IdempotencyKey::new(
                format!("logical-signed-{seq}"),
                InvocationLimits::default(),
            )
            .unwrap(),
            transaction: Some("IMS-GENERIC".into()),
        }),
        system: None,
        q_class: None,
    }
}

fn exercise(store: Arc<dyn PlatformStore>, configuration: ServerConfig) {
    let trust = Arc::new(test_package_trust());
    let server = ProductServer::open_with_package_trust(
        configuration.clone(),
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust.clone(),
    )
    .unwrap();
    let mut package = signed_ims_package(&trust, 1, 1);
    package.sections.ims_metadata = Some(logical_metadata());
    resign_package(&mut package, &trust);
    let installed = server.install_application_package_v2(&package).unwrap();
    server.publish_application_generation(&installed).unwrap();
    server
        .bootstrap_administrator("IBMUSER", b"TESTPASS")
        .unwrap();
    for (class, resource) in [
        ("IMSPSB", "AUTHPSB"),
        ("IMSDB", "AUTHDB"),
        ("IMSDB", "PARENTDB"),
        ("IMSDB", "FILEDB"),
    ] {
        server
            .racf
            .define_profile(class, resource, "IBMUSER", Some(AccessIntent::Control))
            .unwrap();
    }
    let mut inv = tm_invocation("logical-signed", "logical-signed");
    inv.service_class = ServiceClass::Batch;
    inv.deadline_tick = server.jes_clock.now_tick().unwrap() + 60_000;
    inv.principal = Principal::new(
        PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap(),
        [CapabilityId::new("host.ims.write", InvocationLimits::default()).unwrap()]
            .into_iter()
            .collect(),
        InvocationLimits::default(),
    )
    .unwrap();
    let exec = |r: &ImsRequest| {
        server
            .ims_execute_selected("SIGNED-IMS-APPLICATION", &inv, r)
            .unwrap()
    };
    assert_eq!(
        exec(&navigation(ImsOperation::Schedule, 1, 1, None, b"")).status,
        "  "
    );
    for (seq, pcb, name, data, qualifiers) in [
        (2, 2, "DROOT", b"P9A".as_slice(), vec![]),
        (
            3,
            2,
            "LPARENT",
            b"L2X",
            vec![qual("DROOT", "DROOTKEY", b"P9")],
        ),
        (4, 2, "DROOT", b"Q8B", vec![]),
        (
            5,
            2,
            "LPARENT",
            b"L2Y",
            vec![qual("DROOT", "DROOTKEY", b"Q8")],
        ),
        (6, 1, "ROOT", b"A1X", vec![]),
        (
            7,
            1,
            "CHILD",
            b"C1Z",
            vec![
                qual("ROOT", "ROOTKEY", b"A1"),
                qual("LPARENT", "KIND", b"X"),
            ],
        ),
    ] {
        let mut r = navigation(ImsOperation::Insert, seq, pcb, Some(name), data);
        r.qualifiers = qualifiers;
        assert_eq!(exec(&r).status, "  ", "seed {seq}");
    }
    assert_eq!(
        exec(&navigation(ImsOperation::Commit, 8, 1, None, b"")).status,
        "  "
    );
    let get = ImsPcbFeedbackRequestV1 {
        request: navigation(ImsOperation::GetUnique, 9, 1, None, b""),
        context: ImsExecutionContext::DbBatch,
        ssas: Some(vec![b"CHILD   *C(A1C1)".to_vec()]),
        key_capacity: 4,
    };
    let got = server
        .ims_pcb_feedback_selected_v1("SIGNED-IMS-APPLICATION", &inv, &get)
        .unwrap();
    assert_eq!(got.result.status, "  ");
    assert_eq!(got.result.segments[0].data, b"C1ZL2X");
    assert_eq!(
        got.feedback.key,
        ImsPcbKeyFeedbackV1::Valid {
            segment_name: "CHILD".into(),
            segment_level: 2,
            bytes: b"A1C1".to_vec()
        }
    );
    assert_eq!(got.feedback.transferred_data_length, 6);
    assert_eq!(got.feedback.database, "AUTHDB");
    assert_eq!(got.feedback.processing_options, "AP");
    assert_eq!(got.feedback.sensitive_segment_count, 2);

    let recovery = |seq, call| {
        HostRequest::ImsRecovery(recovery_request(
            &installed.identity,
            seq,
            "logical-signed-recovery",
            call,
        ))
    };
    let fb = |seq, op, pcb, segment: &str| {
        HostRequest::ImsPcbFeedbackV1(ImsPcbFeedbackRequestV1 {
            request: navigation(op, seq, pcb, Some(segment), b""),
            context: ImsExecutionContext::DbBatch,
            ssas: None,
            key_capacity: 4,
        })
    };
    let gs = |seq, op, pcb, data: &[u8]| {
        HostRequest::ImsGsam(ImsGsamRequest {
            request: navigation(op, seq, pcb, None, data),
            context: ImsExecutionContext::DbBatch,
            save_address: true,
            search: None,
            undefined_length: None,
        })
    };
    let mut requests = vec![recovery(
        10,
        ImsRecoveryCall::Restart {
            selection: ImsRestartSelection::Normal,
            area_lengths: vec![],
        },
    )];
    let mut seq = 11;
    for op in [
        ImsOperation::GetUnique,
        ImsOperation::GetHoldUnique,
        ImsOperation::GetNext,
        ImsOperation::GetHoldNext,
        ImsOperation::GetNextParent,
        ImsOperation::GetHoldNextParent,
    ] {
        requests.push(fb(seq, ImsOperation::GetUnique, 1, "ROOT"));
        seq += 1;
        requests.push(fb(seq, op, 1, "CHILD"));
        seq += 1;
    }
    requests.extend([
        fb(23, ImsOperation::GetUnique, 4, "CHILD"),
        fb(24, ImsOperation::GetHoldUnique, 1, "CHILD"),
        fb(25, ImsOperation::GetUnique, 3, "ROOT"),
        fb(26, ImsOperation::GetHoldUnique, 5, "ROOT"),
        gs(27, ImsOperation::Insert, 7, b"ONE"),
        gs(28, ImsOperation::Insert, 7, b"TWO"),
        gs(29, ImsOperation::GetNext, 6, b""),
        recovery(
            30,
            ImsRecoveryCall::SymbolicCheckpoint {
                id: "LOGKEY".into(),
                user_areas: vec![b"SAVE".to_vec()],
            },
        ),
    ]);
    let results = run_recovery_machine(&server, store.clone(), &inv, requests.clone());
    assert!(matches!(
        &results[0].outcome,
        Ok(HostResult::ImsRecovery(ImsRecoveryResult::Restarted { .. }))
    ));
    for result in results[1..13].as_chunks::<2>().0 {
        assert_physical(&result[0], "ROOT", 1, b"A1", b"A1X");
        assert_physical(&result[1], "CHILD", 2, b"A1C1", b"C1ZL2X");
    }
    assert_physical(&results[13], "CHILD", 2, b"A1C1", b"");
    assert_physical(&results[14], "CHILD", 2, b"A1C1", b"C1ZL2X");
    assert_physical(&results[15], "ROOT", 1, b"A1", b"A1X");
    assert!(
        matches!(&results[16].outcome, Ok(HostResult::ImsPcbFeedbackV1(r)) if r.result.segments[0].data == b"A1X" && r.feedback.key == ImsPcbKeyFeedbackV1::Unsupported(mainframe_env_host_api::ImsPcbFeedbackUnsupportedV1::SecondarySequence))
    );
    assert!(
        matches!(&results[19].outcome, Ok(HostResult::ImsGsam(r)) if r.result.segments[0].data == b"ONE" && r.address.is_some())
    );
    assert!(matches!(
        &results[20].outcome,
        Ok(HostResult::ImsRecovery(
            ImsRecoveryResult::Checkpointed { .. }
        ))
    ));
    for (r, result) in requests.iter().zip(&results) {
        let journal = store
            .effect(&r.mutation().unwrap().idempotency_key)
            .unwrap()
            .unwrap();
        assert_eq!(journal.state, EffectState::Completed);
        assert_eq!(
            journal.request_digest,
            mainframe_env_host_api::canonical_request_digest(r).unwrap()
        );
        assert_eq!(
            journal.result_digest,
            Some(mainframe_env_host_api::canonical_result_digest(&result.outcome).unwrap())
        );
    }
    drop(server);
    let server = ProductServer::open_with_package_trust(
        configuration,
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust,
    )
    .unwrap();
    let mut next = inv.clone();
    next.execution_id =
        ExecutionId::new("logical-signed-restart", InvocationLimits::default()).unwrap();
    next.deadline_tick = server.jes_clock.now_tick().unwrap() + 60_000;
    let restart = run_recovery_machine(
        &server,
        store.clone(),
        &next,
        vec![recovery(
            31,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Checkpoint("LOGKEY".into()),
                area_lengths: vec![4],
            },
        )],
    );
    assert!(
        matches!(&restart[0].outcome, Ok(HostResult::ImsRecovery(ImsRecoveryResult::Restarted { user_areas, pcb_statuses, .. })) if user_areas == &vec![b"SAVE".to_vec()] && pcb_statuses.len() == 7 && pcb_statuses.iter().all(|(_, status)| status == "  "))
    );
    assert_restored_position(&*store, 1, &[b"A1X", b"C1Z"]);
    assert_restored_position(&*store, 3, &[b"A1X"]);
    assert_restored_position(&*store, 5, &[b"A1X"]);
    next.execution_id =
        ExecutionId::new("logical-signed-continue", InvocationLimits::default()).unwrap();
    let restarted = run_recovery_machine(
        &server,
        store.clone(),
        &next,
        vec![
            gs(32, ImsOperation::GetNext, 6, b""),
            fb(33, ImsOperation::GetNext, 3, "CHILD"),
            HostRequest::ImsPcbFeedbackV1(ImsPcbFeedbackRequestV1 {
                request: navigation(ImsOperation::GetUnique, 34, 1, None, b""),
                context: ImsExecutionContext::DbBatch,
                ssas: Some(vec![b"CHILD   *C(A1C1)".to_vec()]),
                key_capacity: 4,
            }),
            fb(35, ImsOperation::GetNext, 5, "ROOT"),
        ],
    );
    assert!(
        matches!(&restarted[0].outcome, Ok(HostResult::ImsGsam(r)) if r.result.segments[0].data == b"TWO")
    );
    assert_physical(&restarted[1], "CHILD", 2, b"A1C1", b"C1ZL2X");
    assert_physical(&restarted[2], "CHILD", 2, b"A1C1", b"C1ZL2X");
    assert!(
        matches!(&restarted[3].outcome, Ok(HostResult::ImsPcbFeedbackV1(r)) if r.result.status == "GE" && r.feedback.key == ImsPcbKeyFeedbackV1::Unsupported(mainframe_env_host_api::ImsPcbFeedbackUnsupportedV1::FailedCallWitness))
    );
    // Update actual source and destination through their separate retained PCB
    // owners. Exact replay still carries the old data, key and transfer length.
    next.execution_id =
        ExecutionId::new("logical-signed-mutate", InvocationLimits::default()).unwrap();
    let later = run_recovery_machine(
        &server,
        store.clone(),
        &next,
        vec![
            fb(36, ImsOperation::GetHoldUnique, 1, "CHILD"),
            HostRequest::Ims(navigation(ImsOperation::Replace, 37, 1, None, b"C1Q")),
            HostRequest::Ims(navigation(ImsOperation::Commit, 38, 1, None, b"")),
            fb(39, ImsOperation::GetHoldUnique, 2, "LPARENT"),
            HostRequest::Ims(navigation(ImsOperation::Replace, 40, 2, None, b"L2R")),
            HostRequest::Ims(navigation(ImsOperation::Commit, 41, 2, None, b"")),
            fb(42, ImsOperation::GetUnique, 1, "CHILD"),
        ],
    );
    assert!(later.iter().all(
        |r| matches!(&r.outcome, Ok(HostResult::Ims(r)) if r.status == "  ")
            || matches!(&r.outcome, Ok(HostResult::ImsPcbFeedbackV1(r)) if r.result.status == "  ")
    ));
    assert_physical(&later[6], "CHILD", 2, b"A1C1", b"C1QL2R");
    assert_eq!(
        server.ims_pcb_feedback_selected_v1("SIGNED-IMS-APPLICATION", &next, &get),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(
        server
            .ims_pcb_feedback_selected_v1("SIGNED-IMS-APPLICATION", &inv, &get)
            .unwrap(),
        got
    );
    let HostRequest::ImsPcbFeedbackV1(old_request) = &requests[14] else {
        unreachable!()
    };
    let old = server
        .ims_pcb_feedback_selected_v1("SIGNED-IMS-APPLICATION", &inv, old_request)
        .unwrap();
    assert_eq!(Ok(HostResult::ImsPcbFeedbackV1(old)), results[14].outcome);
}

fn assert_restored_position(store: &dyn PlatformStore, pcb: u16, expected: &[&[u8]]) {
    use mainframe_env_ims::database::{DatabaseEngine, EngineLimits, PcbPosition};
    let row = store
        .get_provider_state("ims-v1-session-index", "logical-signed")
        .unwrap()
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
    let position: PcbPosition = serde_json::from_value(if pcb == 1 {
        json["value"]["position"].clone()
    } else {
        json["value"]["pcb_positions"][pcb.to_string()].clone()
    })
    .unwrap();
    assert!(!position.is_held());
    let row = store
        .get_provider_state("ims-v1-generic-database", "AUTHDB")
        .unwrap()
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
    let engine = DatabaseEngine::restore(
        serde_json::from_value(json["value"].clone()).unwrap(),
        EngineLimits::default(),
    )
    .unwrap();
    let path = engine.path_to(position.current().unwrap()).unwrap();
    assert_eq!(
        path.iter()
            .map(|view| view.data.as_slice())
            .collect::<Vec<_>>(),
        expected
    );
}

fn assert_physical(result: &EffectResult, name: &str, level: u16, key: &[u8], data: &[u8]) {
    let Ok(HostResult::ImsPcbFeedbackV1(result)) = &result.outcome else {
        panic!("physical feedback: {result:?}")
    };
    assert_eq!(result.result.status, "  ");
    assert_eq!(
        result.feedback.key,
        ImsPcbKeyFeedbackV1::Valid {
            segment_name: name.into(),
            segment_level: level,
            bytes: key.into()
        }
    );
    assert_eq!(result.feedback.transferred_data_length, data.len() as u64);
    if data.is_empty() {
        assert!(result.result.segments.is_empty());
    } else {
        assert_eq!(result.result.segments.len(), 1);
        assert_eq!(result.result.segments[0].data, data);
    }
}

fn qual(segment: &str, field: &str, value: &[u8]) -> ImsQualifier {
    ImsQualifier {
        segment: segment.into(),
        field: field.into(),
        value: value.into(),
    }
}

#[test]
fn logical_feedback_signed_memory_physical_child() {
    exercise(Arc::new(MemoryStore::new(Default::default())), config());
}
#[test]
fn logical_feedback_signed_sqlite_physical_child() {
    let file = std::env::temp_dir().join(format!("logical-signed-{}.sqlite", std::process::id()));
    let url = format!("sqlite:{}?mode=rwc", file.display());
    let mut c = config();
    c.store_profile = crate::StoreProfile::Sqlite;
    c.sqlite_url = url.clone();
    exercise(
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).unwrap()),
        c,
    );
    std::fs::remove_file(file).unwrap();
}
