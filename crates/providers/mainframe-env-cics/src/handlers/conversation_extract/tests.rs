use crate::conversation_protocol::{
    ConversationAttachHeader, ConversationKind, ConversationLedger, ConversationOwner,
    ConversationSystemDefinition,
};

fn install_extract_fixture(
    cics: &CicsService,
    store: &dyn ProviderStateStore,
    invocation: &Invocation,
) -> ([u8; 4], [u8; 4], [u8; 4]) {
    let owner = ConversationOwner {
        execution: invocation.execution_id.as_str().into(),
        run_unit: invocation.run_unit_id.as_str().into(),
        lease_epoch: u64::from(invocation.attempt),
    };
    let initial = ConversationLedger::load(store).unwrap();
    let mut ledger = initial.clone();
    for (sysid, kind) in [
        ("APP1", ConversationKind::AppcMapped),
        ("MRO1", ConversationKind::Mro),
        ("LU61", ConversationKind::LuType61),
    ] {
        ledger.register_system(ConversationSystemDefinition {
            sysid: sysid.into(), kind, capacity: 4, enabled: true,
        }).unwrap();
    }
    assert_eq!(
        ledger.allocate("LU61", ConversationKind::AppcMapped, owner.clone()),
        Err(crate::ConversationProblem::WrongKind),
    );
    let mapped = ledger.allocate("APP1", ConversationKind::AppcMapped, owner.clone()).unwrap().token;
    let basic = ledger.allocate("APP1", ConversationKind::AppcBasic, owner.clone()).unwrap().token;
    let lu = ledger.allocate("LU61", ConversationKind::LuType61, owner.clone()).unwrap().token;
    let mro = ledger.allocate("MRO1", ConversationKind::Mro, owner.clone()).unwrap().token;
    ledger.conversation_mut(mapped).unwrap().connect(
        &owner, crate::ConversationContext::Local, false,
        b"ORDR".to_vec(), vec![0, 4, 0, 0], 2,
    ).unwrap();
    ledger.conversation_mut(basic).unwrap().connect(
        &owner, crate::ConversationContext::Local, true,
        b"BASICP".to_vec(), vec![0, 4, 0, 0], 1,
    ).unwrap();
    ledger.conversation_mut(mapped).unwrap().principal_facility = true;
    ledger.set_attach(ConversationAttachHeader {
        owner,
        name: "HDR1".into(),
        process: b"TRNX".to_vec(),
        resource: b"RES1".to_vec(),
        return_process: b"RTRN".to_vec(),
        return_resource: b"RRES".to_vec(),
        queue: b"QUEUE".to_vec(),
        iu_type: 1,
        data_stream: 0,
        record_format: 4,
    }).unwrap();
    assert!(initial.persist(&mut ledger, store).unwrap());
    let mut metadata = handlers::ExtractMetadata::for_run_unit(invocation.run_unit_id.as_str().into());
    metadata.session_names.insert("L1".into(), lu);
    metadata.session_names.insert("M1".into(), mro);
    metadata.netnames.insert("LUNAME01".into(), handlers::LuName {
        token: lu, sysid: "LU61".into(), termid: "T001".into(),
    });
    metadata.received_attach = Some("HDR1".into());
    metadata.network_attached = true;
    metadata.logon_message = Some(b"HELLO".to_vec());
    handlers::publish_metadata(cics, metadata, None).unwrap();
    (mapped, basic, lu)
}

fn extract_call(
    cics: &CicsService,
    run: &RunUnitId,
    operation: CicsOperation,
    arguments: BTreeMap<String, BoundedPayload>,
    sequence: u64,
) -> CicsResponse {
    let command = request(operation, arguments, sequence);
    cics.invoke(&effect(run, command.clone(), sequence), command).unwrap()
}

#[test]
fn conversation_extract_reads_shared_ledger_and_positions_owned_lu() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let cics = service(store.clone());
    let invocation = invocation_for("extract-shared", BTreeMap::new());
    let session = SessionId::new("extract-shared", 64).unwrap();
    cics.create_session(&session, 24, 80).unwrap();
    cics.register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS").unwrap();
    let (mapped, basic, lu) = install_extract_fixture(&cics, store.as_ref(), &invocation);
    let process = extract_call(
        &cics, &invocation.run_unit_id, CicsOperation::ExtractProcess,
        BTreeMap::from([
            ("PROCNAME".into(), argument(b"PROC-X")),
            ("PROCNAME.MAXLENGTH".into(), cics_decimal(8)),
            ("PROCLENGTH".into(), argument(b"LEN-X")),
            ("MAXPROCLEN".into(), cics_decimal(8)),
            ("SYNCLEVEL".into(), argument(b"SYNC-X")),
            ("PIPLIST".into(), argument(b"PIP-X")),
            ("PIPLIST.MAXLENGTH".into(), cics_decimal(16)),
            ("PIPLENGTH".into(), argument(b"PIPLEN-X")),
        ]), 1,
    );
    assert_eq!(process.outputs["PROCNAME"].bytes(), b"ORDR    ");
    assert_eq!(process.outputs["PROCLENGTH"].bytes(), b"4");
    assert_eq!(process.outputs["SYNCLEVEL"].bytes(), b"2");
    assert_eq!(process.outputs["PIPLIST"].bytes(), [0, 4, 0, 0]);
    assert_eq!(process.outputs["PIPLENGTH"].bytes(), b"4");
    let attach = extract_call(
        &cics, &invocation.run_unit_id, CicsOperation::ExtractAttach,
        BTreeMap::from([
            ("ATTACHID".into(), cics_literal(b"HDR1")),
            ("PROCESS".into(), argument(b"PROC-X")),
            ("PROCESS.MAXLENGTH".into(), cics_decimal(8)),
            ("IUTYPE".into(), argument(b"IU-X")),
        ]), 2,
    );
    assert_eq!(attach.outputs["PROCESS"].bytes(), b"TRNX");
    assert_eq!(attach.outputs["IUTYPE"].bytes(), b"1");
    let tct = extract_call(
        &cics, &invocation.run_unit_id, CicsOperation::ExtractTct,
        BTreeMap::from([
            ("NETNAME".into(), cics_literal(b"LUNAME01")),
            ("TERMID".into(), argument(b"TERM-X")),
        ]), 3,
    );
    assert_eq!(tct.outputs["TERMID"].bytes(), b"T001");
    let mro_state = extract_call(
        &cics,
        &invocation.run_unit_id,
        CicsOperation::ExtractAttributes,
        BTreeMap::from([
            ("SESSION".into(), cics_literal(b"M1")),
            ("STATE".into(), argument(b"STATE-X")),
        ]),
        3,
    );
    assert_eq!(mro_state.outputs["STATE"].bytes(), b"82");
    let point = extract_call(
        &cics, &invocation.run_unit_id, CicsOperation::Point,
        BTreeMap::from([("SESSION".into(), cics_literal(b"L1"))]), 4,
    );
    assert_eq!(point.condition, "NORMAL");
    let row = store.get_provider_state("cics-conversation-extract-v1", invocation.run_unit_id.as_str())
        .unwrap().unwrap();
    let metadata: handlers::ExtractMetadata = serde_json::from_slice(&row.payload).unwrap();
    assert_eq!(metadata.selected_token, Some(lu));
    let logon = request(CicsOperation::ExtractLogonMsg, BTreeMap::from([
        ("INTO".into(), argument(b"LOGON-X")),
        ("INTO.MAXLENGTH".into(), cics_decimal(256)),
        ("LENGTH".into(), argument(b"LEN-X")),
    ]), 5);
    let first = cics.invoke(&effect(&invocation.run_unit_id, logon.clone(), 5), logon.clone()).unwrap();
    assert_eq!(first.outputs["INTO"].bytes(), b"HELLO");
    assert_eq!(first.outputs["LENGTH"].bytes(), b"5");
    let replay = cics.invoke(&effect(&invocation.run_unit_id, logon.clone(), 5), logon).unwrap();
    assert_eq!(replay.outputs["INTO"].bytes(), b"HELLO");
    let again = extract_call(
        &cics, &invocation.run_unit_id, CicsOperation::ExtractLogonMsg,
        BTreeMap::from([
            ("INTO".into(), argument(b"LOGON-X")),
            ("INTO.MAXLENGTH".into(), cics_decimal(256)),
            ("LENGTH".into(), argument(b"LEN-X")),
        ]), 6,
    );
    assert_eq!(again.outputs["LENGTH"].bytes(), b"0");
    assert_eq!(again.outputs["INTO"].bytes(), b"");
    let current = store
        .get_provider_state(
            "cics-conversation-extract-v1",
            invocation.run_unit_id.as_str(),
        )
        .unwrap()
        .unwrap();
    let mut presentation =
        handlers::ExtractMetadata::for_run_unit(invocation.run_unit_id.as_str().into());
    let previous: handlers::ExtractMetadata = serde_json::from_slice(&current.payload).unwrap();
    presentation.session_names = previous.session_names;
    presentation.netnames = previous.netnames;
    presentation.received_attach = previous.received_attach;
    presentation.network_attached = previous.network_attached;
    presentation.session_names.insert("L2".into(), lu);
    presentation.logon_message = Some(b"HELLO".to_vec());
    handlers::publish_metadata(&cics, presentation.clone(), Some(current.version)).unwrap();
    let published = store
        .get_provider_state(
            "cics-conversation-extract-v1",
            invocation.run_unit_id.as_str(),
        )
        .unwrap()
        .unwrap();
    let preserved: handlers::ExtractMetadata = serde_json::from_slice(&published.payload).unwrap();
    assert_eq!(preserved.selected_token, Some(lu));
    assert!(preserved.logon_consumed);
    assert_eq!(preserved.session_names["L2"], lu);
    let before_rejection = published.clone();
    presentation.logon_message = Some(b"OTHER".to_vec());
    assert_eq!(
        handlers::publish_metadata(&cics, presentation, Some(published.version)),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(
        store
            .get_provider_state(
                "cics-conversation-extract-v1",
                invocation.run_unit_id.as_str(),
            )
            .unwrap()
            .unwrap(),
        before_rejection
    );
    assert_ne!(mapped, basic);
}

#[test]
fn conversation_extract_attach_reads_received_mro_header_and_reports_missing_header() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let cics = service(store.clone());
    let invocation = invocation_for("extract-mro-attach", BTreeMap::new());
    let session = SessionId::new("extract-mro-attach", 64).unwrap();
    cics.create_session(&session, 24, 80).unwrap();
    cics.register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
        .unwrap();
    let (mapped, _, _) = install_extract_fixture(&cics, store.as_ref(), &invocation);
    let metadata_row = store
        .get_provider_state(
            "cics-conversation-extract-v1",
            invocation.run_unit_id.as_str(),
        )
        .unwrap()
        .unwrap();
    let metadata: handlers::ExtractMetadata = serde_json::from_slice(&metadata_row.payload).unwrap();
    let mro = metadata.session_names["M1"];
    let before = ConversationLedger::load(store.as_ref()).unwrap();
    let mut next = before.clone();
    next.conversation_mut(mapped).unwrap().principal_facility = false;
    next.conversation_mut(mro).unwrap().principal_facility = true;
    assert!(before.persist(&mut next, store.as_ref()).unwrap());
    let arguments = BTreeMap::from([
        ("PROCESS".into(), argument(b"PROC-X")),
        ("PROCESS.MAXLENGTH".into(), cics_decimal(8)),
        ("RESOURCE".into(), argument(b"RES-X")),
        ("RESOURCE.MAXLENGTH".into(), cics_decimal(8)),
        ("RPROCESS".into(), argument(b"RPROC-X")),
        ("RPROCESS.MAXLENGTH".into(), cics_decimal(8)),
        ("RRESOURCE".into(), argument(b"RRES-X")),
        ("RRESOURCE.MAXLENGTH".into(), cics_decimal(8)),
        ("QUEUE".into(), argument(b"QUEUE-X")),
        ("QUEUE.MAXLENGTH".into(), cics_decimal(8)),
        ("IUTYPE".into(), argument(b"IU-X")),
        ("DATASTR".into(), argument(b"DATA-X")),
        ("RECFM".into(), argument(b"RECFM-X")),
    ]);
    let response = extract_call(
        &cics,
        &invocation.run_unit_id,
        CicsOperation::ExtractAttach,
        arguments.clone(),
        1,
    );
    for (name, expected) in [
        ("PROCESS", b"TRNX".as_slice()),
        ("RESOURCE", b"RES1".as_slice()),
        ("RPROCESS", b"RTRN".as_slice()),
        ("RRESOURCE", b"RRES".as_slice()),
        ("QUEUE", b"QUEUE".as_slice()),
        ("IUTYPE", b"1".as_slice()),
        ("DATASTR", b"0".as_slice()),
        ("RECFM", b"4".as_slice()),
    ] {
        assert_eq!(response.outputs[name].bytes(), expected, "{name}");
    }
    let mut missing: handlers::ExtractMetadata = serde_json::from_slice(&metadata_row.payload).unwrap();
    missing.received_attach = None;
    handlers::publish_metadata(&cics, missing, Some(metadata_row.version)).unwrap();
    let mut command = request(CicsOperation::ExtractAttach, arguments, 2);
    command.condition_policy = CicsConditionPolicy::Respond {
        response_field: "RESP-X".into(),
        response2_field: Some("RESP2-X".into()),
    };
    let response = cics
        .invoke(&effect(&invocation.run_unit_id, command.clone(), 2), command)
        .unwrap();
    assert_eq!((response.condition.as_str(), response.response), ("CBIDERR", 62));
}

#[test]
fn conversation_process_conditions_and_gds_return_codes_are_distinct() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let cics = service(store.clone());
    let invocation = invocation_for("extract-condition", BTreeMap::new());
    let session = SessionId::new("extract-condition", 64).unwrap();
    cics.create_session(&session, 24, 80).unwrap();
    cics.register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS").unwrap();
    let (mapped, basic, _) = install_extract_fixture(&cics, store.as_ref(), &invocation);
    let mut mapped_state = request(
        CicsOperation::ExtractAttributes,
        BTreeMap::from([
            ("CONVID".into(), cics_literal(&mapped)),
            ("STATE".into(), argument(b"STATE-X")),
        ]),
        1,
    );
    mapped_state.condition_policy = CicsConditionPolicy::Respond {
        response_field: "RESP-X".into(),
        response2_field: Some("RESP2-X".into()),
    };
    let mapped_state = cics
        .invoke(
            &effect(&invocation.run_unit_id, mapped_state.clone(), 1),
            mapped_state,
        )
        .unwrap();
    assert_eq!(mapped_state.outputs["STATE"].bytes(), b"91");
    let mut too_short = request(CicsOperation::ExtractProcess, BTreeMap::from([
        ("PROCNAME".into(), argument(b"NAME-X")),
        ("PROCNAME.MAXLENGTH".into(), cics_decimal(4)),
        ("PROCLENGTH".into(), argument(b"LEN-X")),
        ("MAXPROCLEN".into(), cics_decimal(3)),
    ]), 1);
    too_short.condition_policy = CicsConditionPolicy::Respond {
        response_field: "RESP-X".into(), response2_field: Some("RESP2-X".into()),
    };
    let short = cics.invoke(&effect(&invocation.run_unit_id, too_short.clone(), 1), too_short).unwrap();
    assert_eq!((short.condition.as_str(), short.response), ("LENGERR", 22));
    let basic_result = extract_call(
        &cics, &invocation.run_unit_id, CicsOperation::GdsExtractProcess,
        BTreeMap::from([
            ("CONVID".into(), cics_literal(&basic)),
            ("RETCODE".into(), argument(b"RC-X")),
            ("PROCNAME".into(), argument(b"NAME-X")),
            ("PROCNAME.MAXLENGTH".into(), cics_decimal(8)),
            ("PROCLENGTH".into(), argument(b"LEN-X")),
            ("MAXPROCLEN".into(), cics_decimal(8)),
        ]), 2,
    );
    assert_eq!(basic_result.condition, "NORMAL");
    assert_eq!(basic_result.outputs["RETCODE"].bytes(), [3, 0, 0, 0, 0, 0]);
    let before = ConversationLedger::load(store.as_ref()).unwrap();
    let mut promoted = before.clone();
    promoted.conversation_mut(mapped).unwrap().principal_facility = false;
    promoted.conversation_mut(basic).unwrap().principal_facility = true;
    assert!(before.persist(&mut promoted, store.as_ref()).unwrap());
    let basic_success = extract_call(
        &cics, &invocation.run_unit_id, CicsOperation::GdsExtractProcess,
        BTreeMap::from([
            ("CONVID".into(), cics_literal(&basic)),
            ("RETCODE".into(), argument(b"RC-X")),
            ("PROCNAME".into(), argument(b"NAME-X")),
            ("PROCNAME.MAXLENGTH".into(), cics_decimal(8)),
            ("PROCLENGTH".into(), argument(b"LEN-X")),
            ("MAXPROCLEN".into(), cics_decimal(8)),
            ("SYNCLEVEL".into(), argument(b"SYNC-X")),
        ]), 3,
    );
    assert_eq!(basic_success.outputs["RETCODE"].bytes(), [0; 6]);
    assert_eq!(basic_success.outputs["PROCNAME"].bytes(), b"BASICP  ");
    assert_eq!(basic_success.outputs["PROCLENGTH"].bytes(), b"6");
    assert_eq!(basic_success.outputs["SYNCLEVEL"].bytes(), b"1");
    let indicators = extract_call(
        &cics,
        &invocation.run_unit_id,
        CicsOperation::GdsExtractAttributes,
        BTreeMap::from([
            ("CONVID".into(), cics_literal(&basic)),
            ("CONVDATA".into(), argument(b"DATA-X")),
            ("RETCODE".into(), argument(b"RC-X")),
        ]),
        4,
    );
    assert_eq!(indicators.outputs["RETCODE"].bytes(), [0; 6]);
    assert_eq!(indicators.outputs["CONVDATA"].bytes(), [0; 24]);
    let mut indicators_with_state = request(
        CicsOperation::GdsExtractAttributes,
        BTreeMap::from([
            ("CONVID".into(), cics_literal(&basic)),
            ("CONVDATA".into(), argument(b"DATA-X")),
            ("STATE".into(), argument(b"STATE-X")),
            ("RETCODE".into(), argument(b"RC-X")),
        ]),
        4,
    );
    indicators_with_state.condition_policy = CicsConditionPolicy::Respond {
        response_field: "RESP-X".into(),
        response2_field: Some("RESP2-X".into()),
    };
    let with_state = cics
        .invoke(
            &effect(&invocation.run_unit_id, indicators_with_state.clone(), 4),
            indicators_with_state,
        )
        .unwrap();
    assert_eq!(with_state.outputs["STATE"].bytes(), b"91");
    assert_eq!(with_state.outputs["CONVDATA"].bytes(), [0; 24]);
    assert_eq!(with_state.outputs["RETCODE"].bytes(), [0; 6]);
    let bad_kind = extract_call(
        &cics, &invocation.run_unit_id, CicsOperation::GdsExtractAttributes,
        BTreeMap::from([
            ("CONVID".into(), cics_literal(&mapped)),
            ("CONVDATA".into(), argument(b"DATA-X")),
            ("RETCODE".into(), argument(b"RC-X")),
        ]), 5,
    );
    assert_eq!(bad_kind.outputs["RETCODE"].bytes(), [3, 4, 0, 0, 0, 0]);
    let unowned = extract_call(
        &cics, &invocation.run_unit_id, CicsOperation::GdsExtractAttributes,
        BTreeMap::from([
            ("CONVID".into(), cics_literal(b"BAD1")),
            ("CONVDATA".into(), argument(b"DATA-X")),
            ("RETCODE".into(), argument(b"RC-X")),
        ]), 6,
    );
    assert_eq!(unowned.outputs["RETCODE"].bytes(), [4, 0, 0, 0, 0, 0]);
}

#[test]
fn conversation_extract_process_returns_mapped_pip_beyond_basic_limit() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let cics = service(store.clone());
    let invocation = invocation_for("extract-mapped-pip", BTreeMap::new());
    let session = SessionId::new("extract-mapped-pip", 64).unwrap();
    cics.create_session(&session, 24, 80).unwrap();
    cics.register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
        .unwrap();
    let (mapped, _, _) = install_extract_fixture(&cics, store.as_ref(), &invocation);
    let before = ConversationLedger::load(store.as_ref()).unwrap();
    let mut next = before.clone();
    let mut pip = vec![0; 764];
    pip[..2].copy_from_slice(&764u16.to_be_bytes());
    next.conversation_mut(mapped).unwrap().pip = pip.clone();
    assert!(before.persist(&mut next, store.as_ref()).unwrap());
    let response = extract_call(
        &cics,
        &invocation.run_unit_id,
        CicsOperation::ExtractProcess,
        BTreeMap::from([
            ("CONVID".into(), cics_literal(&mapped)),
            ("PIPLIST".into(), argument(b"PIP-X")),
            ("PIPLIST.MAXLENGTH".into(), cics_decimal(32_763)),
            ("PIPLENGTH".into(), argument(b"LEN-X")),
        ]),
        1,
    );
    assert_eq!(response.outputs["PIPLIST"].bytes(), pip);
    assert_eq!(response.outputs["PIPLENGTH"].bytes(), b"764");
}

#[test]
fn conversation_extract_negative_conditions_do_not_change_protocol_or_position() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let cics = service(store.clone());
    let invocation = invocation_for("extract-negative", BTreeMap::new());
    let session = SessionId::new("extract-negative", 64).unwrap();
    cics.create_session(&session, 24, 80).unwrap();
    cics.register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS").unwrap();
    let (_, basic, _) = install_extract_fixture(&cics, store.as_ref(), &invocation);
    let ledger_before = ConversationLedger::load(store.as_ref()).unwrap();
    let initial_metadata = store
        .get_provider_state(
            "cics-conversation-extract-v1",
            invocation.run_unit_id.as_str(),
        )
        .unwrap()
        .unwrap();
    let mut aliases: handlers::ExtractMetadata =
        serde_json::from_slice(&initial_metadata.payload).unwrap();
    aliases.netnames.insert(
        "MISSING1".into(),
        handlers::LuName {
            token: *b"BAD1",
            sysid: "LU61".into(),
            termid: "T001".into(),
        },
    );
    handlers::publish_metadata(&cics, aliases, Some(initial_metadata.version)).unwrap();
    let meta_before = store.get_provider_state(
        "cics-conversation-extract-v1", invocation.run_unit_id.as_str(),
    ).unwrap().unwrap();
    for (index, (operation, arguments, expected)) in [
        (
            CicsOperation::ExtractTct,
            BTreeMap::from([
                ("NETNAME".into(), cics_literal(b"SHORT")),
                ("TERMID".into(), argument(b"TERM-X")),
            ]),
            ("INVREQ", 16),
        ),
        (
            CicsOperation::ExtractTct,
            BTreeMap::from([
                ("NETNAME".into(), cics_literal(b"UNKNOWN1")),
                ("TERMID".into(), argument(b"TERM-X")),
            ]),
            ("INVREQ", 16),
        ),
        (
            CicsOperation::ExtractTct,
            BTreeMap::from([
                ("NETNAME".into(), cics_literal(b"MISSING1")),
                ("TERMID".into(), argument(b"TERM-X")),
            ]),
            ("NOTALLOC", 61),
        ),
        (
            CicsOperation::ExtractAttach,
            BTreeMap::from([
                ("ATTACHID".into(), cics_literal(b"MISSING")),
                ("PROCESS".into(), argument(b"PROC-X")),
                ("PROCESS.MAXLENGTH".into(), cics_decimal(64)),
            ]),
            ("CBIDERR", 62),
        ),
        (
            CicsOperation::ExtractProcess,
            BTreeMap::from([
                ("CONVID".into(), cics_literal(&basic)),
                ("PROCNAME".into(), argument(b"PROC-X")),
                ("PROCNAME.MAXLENGTH".into(), cics_decimal(32)),
                ("PROCLENGTH".into(), argument(b"LEN-X")),
            ]),
            ("INVREQ", 16),
        ),
        (
            CicsOperation::Point,
            BTreeMap::from([("CONVID".into(), cics_literal(b"BAD1"))]),
            ("NOTALLOC", 61),
        ),
    ].into_iter().enumerate() {
        let sequence = index as u64 + 1;
        let mut command = request(operation, arguments, sequence);
        command.condition_policy = CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        };
        let result = cics.invoke(
            &effect(&invocation.run_unit_id, command.clone(), sequence),
            command,
        ).unwrap();
        assert_eq!((result.condition.as_str(), result.response), expected);
    }
    assert_eq!(ConversationLedger::load(store.as_ref()).unwrap(), ledger_before);
    let meta_after = store.get_provider_state(
        "cics-conversation-extract-v1", invocation.run_unit_id.as_str(),
    ).unwrap().unwrap();
    assert_eq!(meta_after.payload, meta_before.payload);
}

#[test]
fn conversation_extract_ambiguous_principal_fails_closed_without_positioning() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let cics = service(store.clone());
    let invocation = invocation_for("extract-principal-ambiguity", BTreeMap::new());
    let session = SessionId::new("extract-principal-ambiguity", 64).unwrap();
    cics.create_session(&session, 24, 80).unwrap();
    cics.register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
        .unwrap();
    let (mapped, basic, _) = install_extract_fixture(&cics, store.as_ref(), &invocation);
    let before = ConversationLedger::load(store.as_ref()).unwrap();
    let mut ambiguous = before.clone();
    ambiguous.conversation_mut(basic).unwrap().principal_facility = true;
    assert!(before.persist(&mut ambiguous, store.as_ref()).unwrap());
    let sidecar_before = store
        .get_provider_state(
            "cics-conversation-extract-v1",
            invocation.run_unit_id.as_str(),
        )
        .unwrap()
        .unwrap();
    let mut attributes = request(
        CicsOperation::ExtractAttributes,
        BTreeMap::from([("STATE".into(), argument(b"STATE-X"))]),
        1,
    );
    attributes.condition_policy = CicsConditionPolicy::Respond {
        response_field: "RESP-X".into(),
        response2_field: Some("RESP2-X".into()),
    };
    assert_eq!(
        cics.invoke(
            &effect(&invocation.run_unit_id, attributes.clone(), 1),
            attributes,
        ),
        Err(HostProblem::InfrastructureFailure)
    );
    let point = request(CicsOperation::Point, BTreeMap::new(), 2);
    assert_eq!(
        cics.invoke(&effect(&invocation.run_unit_id, point.clone(), 2), point),
        Err(HostProblem::InfrastructureFailure)
    );
    assert_eq!(
        store
            .get_provider_state(
                "cics-conversation-extract-v1",
                invocation.run_unit_id.as_str(),
            )
            .unwrap()
            .unwrap(),
        sidecar_before
    );
    let explicit = extract_call(
        &cics,
        &invocation.run_unit_id,
        CicsOperation::ExtractAttributes,
        BTreeMap::from([
            ("CONVID".into(), cics_literal(&mapped)),
            ("STATE".into(), argument(b"STATE-X")),
        ]),
        3,
    );
    assert_eq!(explicit.outputs["STATE"].bytes(), b"91");
}

#[test]
fn conversation_extract_dpl_principal_restrictions_are_command_specific() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let cics = service(store.clone());
    let invocation = invocation_for(
        "extract-dpl",
        BTreeMap::from([(
            "cics.execution-context".into(),
            BoundedPayload::new(
                "mainframe-env.cics.execution-context@1",
                b"dpl-without-synconreturn".to_vec(),
                InvocationLimits::default(),
            ).unwrap(),
        )]),
    );
    let session = SessionId::new("extract-dpl", 64).unwrap();
    cics.create_session(&session, 24, 80).unwrap();
    cics.register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS").unwrap();
    let (mapped, basic, _) = install_extract_fixture(&cics, store.as_ref(), &invocation);
    let mut process = request(
        CicsOperation::ExtractProcess,
        BTreeMap::from([
            ("CONVID".into(), cics_literal(&mapped)),
            ("PROCNAME".into(), argument(b"PROC-X")),
            ("PROCNAME.MAXLENGTH".into(), cics_decimal(32)),
            ("PROCLENGTH".into(), argument(b"LEN-X")),
        ]),
        1,
    );
    process.condition_policy = CicsConditionPolicy::Respond {
        response_field: "RESP-X".into(), response2_field: Some("RESP2-X".into()),
    };
    let rejected = cics.invoke(
        &effect(&invocation.run_unit_id, process.clone(), 1), process,
    ).unwrap();
    assert_eq!(
        (rejected.condition.as_str(), rejected.response, rejected.response2),
        ("INVREQ", 16, 200),
    );
    let point = extract_call(
        &cics, &invocation.run_unit_id, CicsOperation::Point,
        BTreeMap::from([("SESSION".into(), cics_literal(b"L1"))]), 2,
    );
    assert_eq!(point.condition, "NORMAL");
    let old = ConversationLedger::load(store.as_ref()).unwrap();
    let mut next = old.clone();
    next.conversation_mut(mapped).unwrap().principal_facility = false;
    next.conversation_mut(basic).unwrap().principal_facility = true;
    assert!(old.persist(&mut next, store.as_ref()).unwrap());
    let gds = extract_call(
        &cics, &invocation.run_unit_id, CicsOperation::GdsExtractAttributes,
        BTreeMap::from([
            ("CONVID".into(), cics_literal(&basic)),
            ("CONVDATA".into(), argument(b"DATA-X")),
            ("RETCODE".into(), argument(b"RC-X")),
        ]), 3,
    );
    assert_eq!(gds.outputs["RETCODE"].bytes(), [3, 1, 0, 0, 0, 0]);
}

#[test]
fn conversation_extract_point_saf_denial_is_audited_before_selection() {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let secrets = Arc::new(MemorySecretResolver::default());
    secrets.insert("secret:ibmuser", b"PASSWORD".to_vec());
    let racf = RacfService::open(store.clone(), secrets, Default::default()).unwrap();
    racf.add_user(
        "IBMUSER",
        &SecretRef::new("secret:ibmuser", HostLimits::default()).unwrap(),
    )
    .unwrap();
    let host = Arc::new(ScopedHostService::new(
        Arc::new(
            RegistrySnapshot::new(
                1,
                racf_providers(racf.clone(), InvocationLimits::default()),
                InvocationLimits::default(),
            )
            .unwrap(),
        ),
        HostLimits::default(),
    ));
    let cics = CicsService::open(host, store.clone(), CicsLimits::default()).unwrap();
    let invocation = invocation_for("extract-saf", BTreeMap::new());
    let session = SessionId::new("extract-saf", 64).unwrap();
    cics.create_session(&session, 24, 80).unwrap();
    cics.register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
        .unwrap();
    let (_, _, lu) = install_extract_fixture(&cics, store.as_ref(), &invocation);
    let key = invocation.run_unit_id.as_str();
    let before = store
        .get_provider_state("cics-conversation-extract-v1", key)
        .unwrap()
        .unwrap();
    let denied = request(
        CicsOperation::Point,
        BTreeMap::from([("CONVID".into(), cics_literal(&lu))]),
        1,
    );
    assert_eq!(
        cics.invoke(&effect(&invocation.run_unit_id, denied.clone(), 1), denied),
        Err(HostProblem::Unauthorized)
    );
    assert_eq!(
        store
            .get_provider_state("cics-conversation-extract-v1", key)
            .unwrap()
            .unwrap(),
        before
    );
    assert!(store
        .audit_records(&invocation.execution_id, 0, 16)
        .unwrap()
        .iter()
        .any(|record| record.decision == AuditDecision::Deny));

    racf.define_profile(
        "TCICSTRN",
        "CICS.MENU",
        "IBMUSER",
        Some(AccessIntent::Execute),
    )
    .unwrap();
    let permitted = extract_call(
        &cics,
        &invocation.run_unit_id,
        CicsOperation::Point,
        BTreeMap::from([("CONVID".into(), cics_literal(&lu))]),
        2,
    );
    assert_eq!(permitted.condition, "NORMAL");
    let after = store
        .get_provider_state("cics-conversation-extract-v1", key)
        .unwrap()
        .unwrap();
    let metadata: handlers::ExtractMetadata = serde_json::from_slice(&after.payload).unwrap();
    assert_eq!(metadata.selected_token, Some(lu));
}

#[test]
fn conversation_extract_timeout_and_cancellation_leave_metadata_unmodified() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let cics = service(store.clone());
    let invocation = invocation_for("extract-stop", BTreeMap::new());
    let session = SessionId::new("extract-stop", 64).unwrap();
    cics.create_session(&session, 24, 80).unwrap();
    cics.register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
        .unwrap();
    let (_, _, lu) = install_extract_fixture(&cics, store.as_ref(), &invocation);
    let before = store
        .get_provider_state("cics-conversation-extract-v1", invocation.run_unit_id.as_str())
        .unwrap()
        .unwrap();
    let outer = ScopedHostService::new(
        Arc::new(
            RegistrySnapshot::new(
                1,
                vec![cics_provider(cics, InvocationLimits::default())],
                InvocationLimits::default(),
            )
            .unwrap(),
        ),
        HostLimits::default(),
    );
    let point = request(
        CicsOperation::Point,
        BTreeMap::from([("CONVID".into(), cics_literal(&lu))]),
        1,
    );
    assert_eq!(
        outer
            .invoke(
                &invocation,
                invocation.deadline_tick,
                false,
                effect(&invocation.run_unit_id, point.clone(), 1),
            )
            .into_transaction_parts()
            .0
            .outcome,
        Err(HostProblem::TimedOut)
    );
    let logon = request(
        CicsOperation::ExtractLogonMsg,
        BTreeMap::from([
            ("INTO".into(), argument(b"LOGON-X")),
            ("INTO.MAXLENGTH".into(), cics_decimal(256)),
            ("LENGTH".into(), argument(b"LEN-X")),
        ]),
        2,
    );
    assert_eq!(
        outer
            .invoke(
                &invocation,
                1,
                true,
                effect(&invocation.run_unit_id, logon.clone(), 2),
            )
            .into_transaction_parts()
            .0
            .outcome,
        Err(HostProblem::Cancelled)
    );
    assert_eq!(
        store
            .get_provider_state("cics-conversation-extract-v1", invocation.run_unit_id.as_str())
            .unwrap()
            .unwrap(),
        before
    );
}

#[test]
fn conversation_extract_replays_unknown_logon_after_interleaved_point() {
    let store = Arc::new(FailCicsReplayCasStore::new());
    let cics = service(store.clone());
    let invocation = invocation_for("extract-replay-gap", BTreeMap::new());
    let session = SessionId::new("extract-replay-gap", 64).unwrap();
    cics.create_session(&session, 24, 80).unwrap();
    cics.register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
        .unwrap();
    install_extract_fixture(&cics, store.as_ref(), &invocation);
    let key = invocation.run_unit_id.as_str();
    let logon = request(
        CicsOperation::ExtractLogonMsg,
        BTreeMap::from([
            ("INTO".into(), argument(b"LOGON-X")),
            ("INTO.MAXLENGTH".into(), cics_decimal(256)),
            ("LENGTH".into(), argument(b"LEN-X")),
        ]),
        1,
    );
    store.fail_insert.store(true, Ordering::SeqCst);
    assert_eq!(
        cics.invoke(&effect(&invocation.run_unit_id, logon.clone(), 1), logon.clone()),
        Err(HostProblem::UnknownOutcome)
    );
    assert!(store
        .get_provider_state("cics-effect-replay-v1", "outer-1")
        .unwrap()
        .is_none());
    let point = request(
        CicsOperation::Point,
        BTreeMap::from([("SESSION".into(), cics_literal(b"M1"))]),
        2,
    );
    assert_eq!(
        cics.invoke(&effect(&invocation.run_unit_id, point.clone(), 2), point)
            .unwrap()
            .condition,
        "NORMAL"
    );
    let before_retry = store
        .get_provider_state("cics-conversation-extract-v1", key)
        .unwrap()
        .unwrap();
    let metadata: handlers::ExtractMetadata = serde_json::from_slice(&before_retry.payload).unwrap();
    assert_eq!(metadata.selected_token, Some(metadata.session_names["M1"]));
    assert!(metadata.logon_consumed);
    let row: serde_json::Value = serde_json::from_slice(&before_retry.payload).unwrap();
    assert_eq!(row["mutation_replays"].as_array().unwrap().len(), 2);

    let mut conflict = logon.clone();
    conflict
        .arguments
        .insert("INTO".into(), argument(b"OTHER-X"));
    assert_eq!(
        cics.invoke(&effect(&invocation.run_unit_id, conflict.clone(), 1), conflict),
        Err(HostProblem::IdempotencyConflict)
    );
    let recovered = cics
        .invoke(&effect(&invocation.run_unit_id, logon.clone(), 1), logon)
        .unwrap();
    assert_eq!(recovered.outputs["INTO"].bytes(), b"HELLO");
    assert_eq!(recovered.outputs["LENGTH"].bytes(), b"5");
    assert_eq!(
        store
            .get_provider_state("cics-conversation-extract-v1", key)
            .unwrap()
            .unwrap(),
        before_retry
    );

    let later = request(
        CicsOperation::Point,
        BTreeMap::from([("SESSION".into(), cics_literal(b"L1"))]),
        3,
    );
    cics.invoke(&effect(&invocation.run_unit_id, later.clone(), 3), later)
        .unwrap();
    let after = store
        .get_provider_state("cics-conversation-extract-v1", key)
        .unwrap()
        .unwrap();
    let metadata: handlers::ExtractMetadata = serde_json::from_slice(&after.payload).unwrap();
    let row: serde_json::Value = serde_json::from_slice(&after.payload).unwrap();
    assert_eq!(row["mutation_replays"].as_array().unwrap().len(), 1);
    assert_eq!(metadata.selected_token, Some(metadata.session_names["L1"]));
}

#[test]
fn conversation_extract_unknown_reply_survives_sqlite_restart_after_point() {
    let root = std::env::temp_dir().join(format!(
        "mainframe-conv-extract-replay-{}-{:?}",
        std::process::id(),
        std::thread::current().id(),
    ));
    std::fs::create_dir_all(&root).unwrap();
    let url = format!("sqlite://{}?mode=rwc", root.join("state.db").display());
    let store = Arc::new(FailCicsReplayCasStore::with_inner(
        SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap(),
    ));
    let cics = service(store.clone());
    let invocation = invocation_for("extract-sqlite-replay", BTreeMap::new());
    let session = SessionId::new("extract-sqlite-replay", 64).unwrap();
    cics.create_session(&session, 24, 80).unwrap();
    cics.register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
        .unwrap();
    install_extract_fixture(&cics, store.as_ref(), &invocation);
    let logon = request(
        CicsOperation::ExtractLogonMsg,
        BTreeMap::from([
            ("INTO".into(), argument(b"LOGON-X")),
            ("INTO.MAXLENGTH".into(), cics_decimal(256)),
            ("LENGTH".into(), argument(b"LEN-X")),
        ]),
        1,
    );
    store.fail_insert.store(true, Ordering::SeqCst);
    assert_eq!(
        cics.invoke(&effect(&invocation.run_unit_id, logon.clone(), 1), logon.clone()),
        Err(HostProblem::UnknownOutcome)
    );
    let point = request(
        CicsOperation::Point,
        BTreeMap::from([("SESSION".into(), cics_literal(b"M1"))]),
        2,
    );
    cics.invoke(&effect(&invocation.run_unit_id, point.clone(), 2), point)
        .unwrap();
    let before = store
        .get_provider_state(
            "cics-conversation-extract-v1",
            invocation.run_unit_id.as_str(),
        )
        .unwrap()
        .unwrap();
    drop((cics, store));

    let reopened_store = Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
    let reopened = service(reopened_store.clone());
    let resumed_session = SessionId::new("extract-sqlite-replay-resumed", 64).unwrap();
    reopened.create_session(&resumed_session, 24, 80).unwrap();
    reopened
        .register_run(invocation.clone(), &resumed_session, "MENU", "MEAPPL", "MESYS")
        .unwrap();
    let recovered = reopened
        .invoke(&effect(&invocation.run_unit_id, logon.clone(), 1), logon)
        .unwrap();
    assert_eq!(recovered.outputs["INTO"].bytes(), b"HELLO");
    assert_eq!(recovered.outputs["LENGTH"].bytes(), b"5");
    let after = reopened_store
        .get_provider_state(
            "cics-conversation-extract-v1",
            invocation.run_unit_id.as_str(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(after, before);
    let metadata: handlers::ExtractMetadata = serde_json::from_slice(&after.payload).unwrap();
    assert_eq!(metadata.selected_token, Some(metadata.session_names["M1"]));
    assert!(metadata.logon_consumed);
    assert!(reopened_store
        .get_provider_state("cics-effect-replay-v1", "outer-1")
        .unwrap()
        .is_some());
    drop((reopened, reopened_store));
    std::fs::remove_dir_all(root).unwrap();
}
