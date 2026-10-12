use super::super::*;
use mainframe_env_execution_api::{Machine, MachineDrive, MachineResume, Quantum};
use mainframe_env_interpreter::ReferenceMachine;
use mainframe_env_ir::CodecLimits;

fn seeded(duplicate_key: bool) -> (Arc<MemoryStore>, Arc<DatasetService>, DatasetName) {
    let store = Arc::new(MemoryStore::new(StoreLimits::default()));
    let dataset = DatasetService::open(store.clone(), DatasetLimits::default()).unwrap();
    dataset
        .invoke(DatasetRequest::Create {
            dataset: dataset_name().unwrap(),
            attributes: DatasetAttributes {
                organization: DatasetOrganization::KeySequenced,
                record_format: RecordFormat::Fixed,
                logical_record_length: 4,
                key_offset: Some(0),
                key_length: Some(2),
                ccsid: None,
            },
            mutation: mutation(1, "bounded-create").unwrap(),
        })
        .unwrap();
    dataset
        .invoke(DatasetRequest::Write {
            dataset: dataset_name().unwrap(),
            member: None,
            records: vec![b"AA11".to_vec(), b"BB11".to_vec()],
            expected_version: Some(1),
            mutation: mutation(2, "bounded-seed").unwrap(),
        })
        .unwrap();
    let selected = if duplicate_key {
        let index = DatasetName::new("PILOT.AIX", 44).unwrap();
        let path = DatasetName::new("PILOT.PATH", 44).unwrap();
        dataset
            .invoke(DatasetRequest::DefineAlternateIndex {
                base: dataset_name().unwrap(),
                index: index.clone(),
                key_offset: 2,
                key_length: 2,
                allow_duplicates: true,
                upgrade: true,
                mutation: mutation(3, "bounded-aix").unwrap(),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::DefinePath {
                path: path.clone(),
                index,
                mutation: mutation(4, "bounded-path").unwrap(),
            })
            .unwrap();
        path
    } else {
        dataset_name().unwrap()
    };
    (store, dataset, selected)
}

fn read_request(selected: &DatasetName, duplicate_key: bool) -> DatasetRequest {
    DatasetRequest::Read {
        dataset: selected.clone(),
        member: None,
        key: duplicate_key.then(|| b"11".to_vec()),
        max_records: 1,
        control: Default::default(),
    }
}

fn assert_bounded(result: &DatasetResult) {
    assert!(
        matches!(result, DatasetResult::Records { records, identities, more: true, .. }
        if records == &[b"AA11".to_vec()] && identities == &[b"AA".to_vec()])
    );
}

fn cics_call(
    cics: &CicsService,
    invocation: &Invocation,
    operation: CicsOperation,
    arguments: BTreeMap<String, BoundedPayload>,
    sequence: u64,
) -> mainframe_env_host_api::CicsResponse {
    let request = CicsRequest {
        operation,
        arguments: arguments.clone(),
        condition_policy: CicsConditionPolicy::Default,
        mutation: (operation.is_mutating() || arguments.contains_key("TOKEN")).then(|| {
            let mut value = mutation(sequence, &format!("bounded-cics-{sequence}")).unwrap();
            value.transaction = Some("PIL1".into());
            value
        }),
    };
    cics.invoke(
        &EffectRequest {
            run_unit: invocation.run_unit_id.clone(),
            sequence,
            deadline_tick: invocation.deadline_tick,
            idempotency_key: request
                .mutation
                .as_ref()
                .map(|value| value.idempotency_key.clone()),
            request: HostRequest::Cics(request.clone()),
        },
        request,
    )
    .unwrap()
}

fn argument(bytes: &[u8]) -> BoundedPayload {
    BoundedPayload::new(
        "mainframe-env.cics.argument@1",
        bytes.to_vec(),
        InvocationLimits::default(),
    )
    .unwrap()
}

fn cics_single_record_case(duplicate_key: bool) {
    let (store, dataset, selected) = seeded(duplicate_key);
    assert_bounded(
        &dataset
            .invoke(read_request(&selected, duplicate_key))
            .unwrap(),
    );
    let cics = CicsService::open(
        pilot_inner_host(dataset.clone()).unwrap(),
        store,
        CicsLimits::default(),
    )
    .unwrap();
    cics.register_file_definitions(&BTreeMap::from([(
        "ACCTDAT".into(),
        CicsFileDefinition {
            dataset: selected,
            ccsid: None,
        },
    )]))
    .unwrap();
    let artifact = crate::compile(
        "IDENTIFICATION DIVISION. PROGRAM-ID. BOUNDED. PROCEDURE DIVISION. STOP RUN.",
    )
    .unwrap();
    let invocation = pilot_invocation(&artifact, "IBMUSER", "bounded", "BOUNDED").unwrap();
    let session = SessionId::new("bounded-session", 64).unwrap();
    cics.create_session(&session, 24, 80).unwrap();
    cics.register_run(invocation.clone(), &session, "PIL1", "MEAPPL", "MESYS")
        .unwrap();
    let mut arguments = BTreeMap::from([("FILE".into(), argument(b"ACCTDAT"))]);
    if duplicate_key {
        arguments.insert("RIDFLD".into(), argument(b"11"));
    }
    let plain = cics_call(
        &cics,
        &invocation,
        CicsOperation::Read,
        arguments.clone(),
        1,
    );
    assert_eq!((plain.response, plain.response2), (0, 0));
    assert_eq!(plain.payload.bytes(), b"AA11");
    arguments.insert("OPTION.UPDATE".into(), argument(b""));
    if duplicate_key {
        arguments.insert("TOKEN".into(), argument(b""));
    }
    let update = cics_call(&cics, &invocation, CicsOperation::Read, arguments, 2);
    assert_eq!((update.response, update.response2), (0, 0));
    assert_eq!(update.payload.bytes(), b"AA11");
    if duplicate_key {
        // AIX paths support record selection; mutation through a path is not
        // implemented by this provider. Verify the READ UPDATE token instead.
        let token = update.outputs.get("TOKEN").expect("READ UPDATE token");
        assert!(
            std::str::from_utf8(token.bytes())
                .unwrap()
                .parse::<u32>()
                .unwrap()
                > 0
        );
        let unlock = cics_call(
            &cics,
            &invocation,
            CicsOperation::Unlock,
            BTreeMap::from([
                ("FILE".into(), argument(b"ACCTDAT")),
                ("TOKEN".into(), token.clone()),
            ]),
            3,
        );
        assert_eq!((unlock.response, unlock.response2), (0, 0));
        return;
    }
    let rewrite = cics_call(
        &cics,
        &invocation,
        CicsOperation::Rewrite,
        BTreeMap::from([
            ("FILE".into(), argument(b"ACCTDAT")),
            ("FROM".into(), argument(b"AA22")),
        ]),
        3,
    );
    assert_eq!((rewrite.response, rewrite.response2), (0, 0));
    let records = dataset
        .invoke(DatasetRequest::Read {
            dataset: dataset_name().unwrap(),
            member: None,
            key: None,
            max_records: 2,
            control: Default::default(),
        })
        .unwrap();
    assert!(
        matches!(records, DatasetResult::Records { records, more: false, .. }
            if records == [b"AA22".to_vec(), b"BB11".to_vec()])
    );
}

fn interpreter_single_record_case(duplicate_key: bool) {
    let (_, dataset, selected) = seeded(duplicate_key);
    let key = if duplicate_key { "KEY IS ALT-KEY" } else { "" };
    let select = if duplicate_key {
        "SELECT TEST-FILE ASSIGN TO TESTDD ORGANIZATION IS INDEXED ACCESS MODE IS RANDOM RECORD KEY IS REC-KEY ALTERNATE RECORD KEY IS ALT-KEY WITH DUPLICATES FILE STATUS IS STATUS-X."
    } else {
        "SELECT TEST-FILE ASSIGN TO TESTDD ORGANIZATION IS SEQUENTIAL FILE STATUS IS STATUS-X."
    };
    let source = format!(
        "IDENTIFICATION DIVISION. PROGRAM-ID. BOUNDED.
ENVIRONMENT DIVISION. INPUT-OUTPUT SECTION. FILE-CONTROL.
{select}
DATA DIVISION. FILE SECTION. FD TEST-FILE.
01 TEST-RECORD. 05 REC-KEY PIC X(2). 05 ALT-KEY PIC X(2) VALUE '11'.
WORKING-STORAGE SECTION. 01 STATUS-X PIC XX. 01 INTO-X PIC X(4).
PROCEDURE DIVISION. READ TEST-FILE INTO INTO-X {key}.
DISPLAY STATUS-X ':' TEST-RECORD ':' INTO-X. STOP RUN."
    );
    let artifact = crate::compile(&source).unwrap();
    let mut invocation = pilot_invocation(&artifact, "IBMUSER", "bounded", "BOUNDED").unwrap();
    invocation.bindings.insert(
        "cobol.dd.TESTDD".into(),
        argument(selected.as_str().as_bytes()),
    );
    let mut machine =
        ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
            .unwrap();
    let mut resume = MachineResume::Start;
    let mut reads = 0;
    loop {
        match machine.drive(resume, Quantum::new(512, 65_536).unwrap()) {
            MachineDrive::Continue => resume = MachineResume::Start,
            MachineDrive::HostCall(effect) => {
                let HostRequest::Dataset(request) = effect.request else {
                    panic!("expected dataset read")
                };
                assert!(matches!(
                    &request,
                    DatasetRequest::Read { max_records: 1, .. }
                ));
                let result = dataset.invoke(request).unwrap();
                assert_bounded(&result);
                reads += 1;
                resume = MachineResume::HostResult(EffectResult {
                    sequence: effect.sequence,
                    outcome: Ok(HostResult::Dataset(result)),
                });
            }
            MachineDrive::Completed(completion) => {
                assert_eq!(reads, 1);
                assert_eq!(completion.output.bytes(), b"00:AA11:AA11\n");
                break;
            }
            other => panic!("single-record read did not complete: {other:?}"),
        }
    }
}

#[test]
fn cics_keyless_single_record_read_preserves_update_with_more() {
    cics_single_record_case(false);
}

#[test]
fn cics_duplicate_aix_single_record_read_preserves_update_with_more() {
    cics_single_record_case(true);
}

#[test]
fn interpreter_keyless_single_record_read_accepts_more() {
    interpreter_single_record_case(false);
}

#[test]
fn interpreter_duplicate_aix_single_record_read_accepts_more() {
    interpreter_single_record_case(true);
}
