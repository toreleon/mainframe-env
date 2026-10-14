use super::super::*;
use mainframe_env_diagnostics::FailureCategory;
use mainframe_env_execution_api::{Machine, MachineDrive, MachineResume, Quantum};
use mainframe_env_interpreter::ReferenceMachine;
use mainframe_env_ir::CodecLimits;
use mainframe_env_store::PostgresStateStore;

fn unique_namespace() -> String {
    let identity = format!(
        "{}-{:?}-{}",
        std::process::id(),
        std::time::SystemTime::now(),
        PROFILE_NONCE.fetch_add(1, Ordering::Relaxed)
    );
    let hash = digest(identity.as_bytes()).to_ascii_uppercase();
    format!("BR.X{}.X{}.X{}", &hash[7..14], &hash[14..21], &hash[21..28])
}

fn memory_store() -> Arc<dyn ProviderStateStore> {
    Arc::new(MemoryStore::new(StoreLimits::default()))
}

fn postgres_store() -> Arc<dyn ProviderStateStore> {
    let url =
        std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL").expect("isolated PostgreSQL 18 test URL");
    Arc::new(PostgresStateStore::open(&url, 8 * 1024 * 1024, 4096).unwrap())
}

fn sqlite_case(case: fn(Arc<dyn ProviderStateStore>, bool), duplicate_key: bool) {
    let directory = std::env::temp_dir().join(unique_namespace());
    std::fs::create_dir_all(&directory).unwrap();
    let url = format!("sqlite://{}?mode=rwc", directory.join("state.db").display());
    case(
        Arc::new(SqliteStateStore::open(&url, 8 * 1024 * 1024, 4096).unwrap()),
        duplicate_key,
    );
    std::fs::remove_dir_all(directory).unwrap();
}

fn seeded(
    store: Arc<dyn ProviderStateStore>,
    duplicate_key: bool,
) -> (Arc<DatasetService>, DatasetName) {
    // Providers share fixed store namespaces, so isolate every persisted identity.
    let namespace = unique_namespace();
    let base = DatasetName::new(format!("{namespace}.ACCTDAT"), 44).unwrap();
    let dataset = DatasetService::open(store, DatasetLimits::default()).unwrap();
    dataset
        .invoke(DatasetRequest::Create {
            dataset: base.clone(),
            attributes: DatasetAttributes {
                organization: DatasetOrganization::KeySequenced,
                record_format: RecordFormat::Fixed,
                logical_record_length: 4,
                key_offset: Some(0),
                key_length: Some(2),
                ccsid: None,
            },
            mutation: mutation(1, &format!("{namespace}-create")).unwrap(),
        })
        .unwrap();
    dataset
        .invoke(DatasetRequest::Write {
            dataset: base.clone(),
            member: None,
            records: vec![b"AA11".to_vec(), b"BB11".to_vec()],
            expected_version: Some(1),
            mutation: mutation(2, &format!("{namespace}-seed")).unwrap(),
        })
        .unwrap();
    let selected = if duplicate_key {
        let index = DatasetName::new(format!("{namespace}.AIX"), 44).unwrap();
        let path = DatasetName::new(format!("{namespace}.PATH"), 44).unwrap();
        dataset
            .invoke(DatasetRequest::DefineAlternateIndex {
                base: base.clone(),
                index: index.clone(),
                key_offset: 2,
                key_length: 2,
                allow_duplicates: true,
                upgrade: true,
                mutation: mutation(3, &format!("{namespace}-aix")).unwrap(),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::DefinePath {
                path: path.clone(),
                index,
                mutation: mutation(4, &format!("{namespace}-path")).unwrap(),
            })
            .unwrap();
        path
    } else {
        base
    };
    (dataset, selected)
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
            let mut value = mutation(
                sequence,
                &format!("{}-cics-{sequence}", invocation.run_unit_id.as_str()),
            )
            .unwrap();
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

fn cics_single_record_case(store: Arc<dyn ProviderStateStore>, duplicate_key: bool) {
    let (dataset, selected) = seeded(store.clone(), duplicate_key);
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
    let file = format!("F{}", &digest(selected.as_str().as_bytes())[7..22]).to_ascii_uppercase();
    cics.register_file_definitions(&BTreeMap::from([(
        file.clone(),
        CicsFileDefinition {
            dataset: selected.clone(),
            ccsid: None,
        },
    )]))
    .unwrap();
    let artifact = crate::compile(
        "IDENTIFICATION DIVISION. PROGRAM-ID. BOUNDED. PROCEDURE DIVISION. STOP RUN.",
    )
    .unwrap();
    let invocation = pilot_invocation(&artifact, "IBMUSER", selected.as_str(), "BOUNDED").unwrap();
    let session = SessionId::new(
        format!("{}-session", selected.as_str().replace('.', "-")),
        64,
    )
    .unwrap();
    cics.create_session(&session, 24, 80).unwrap();
    cics.register_run(invocation.clone(), &session, "PIL1", "MEAPPL", "MESYS")
        .unwrap();
    let mut arguments = BTreeMap::from([("FILE".into(), argument(file.as_bytes()))]);
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
                ("FILE".into(), argument(file.as_bytes())),
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
            ("FILE".into(), argument(file.as_bytes())),
            ("FROM".into(), argument(b"AA22")),
        ]),
        3,
    );
    assert_eq!((rewrite.response, rewrite.response2), (0, 0));
    let records = dataset
        .invoke(DatasetRequest::Read {
            dataset: selected,
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

fn interpreter_single_record_case(store: Arc<dyn ProviderStateStore>, duplicate_key: bool) {
    let (dataset, selected) = seeded(store, duplicate_key);
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
    let mut invocation =
        pilot_invocation(&artifact, "IBMUSER", selected.as_str(), "BOUNDED").unwrap();
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
    cics_single_record_case(memory_store(), false);
}

#[test]
fn cics_duplicate_aix_single_record_read_preserves_update_with_more() {
    cics_single_record_case(memory_store(), true);
}

#[test]
fn interpreter_keyless_single_record_read_accepts_more() {
    interpreter_single_record_case(memory_store(), false);
}

#[test]
fn interpreter_duplicate_aix_single_record_read_accepts_more() {
    interpreter_single_record_case(memory_store(), true);
}

#[test]
fn cics_keyless_single_record_read_preserves_update_with_more_sqlite() {
    sqlite_case(cics_single_record_case, false);
}

#[test]
fn cics_duplicate_aix_single_record_read_preserves_update_with_more_sqlite() {
    sqlite_case(cics_single_record_case, true);
}

#[test]
fn interpreter_keyless_single_record_read_accepts_more_sqlite() {
    sqlite_case(interpreter_single_record_case, false);
}

#[test]
fn interpreter_duplicate_aix_single_record_read_accepts_more_sqlite() {
    sqlite_case(interpreter_single_record_case, true);
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
fn cics_keyless_single_record_read_preserves_update_with_more_postgres() {
    cics_single_record_case(postgres_store(), false);
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
fn cics_duplicate_aix_single_record_read_preserves_update_with_more_postgres() {
    cics_single_record_case(postgres_store(), true);
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
fn interpreter_keyless_single_record_read_accepts_more_postgres() {
    interpreter_single_record_case(postgres_store(), false);
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
fn interpreter_duplicate_aix_single_record_read_accepts_more_postgres() {
    interpreter_single_record_case(postgres_store(), true);
}

fn interpreter_sort_case(store: Arc<dyn ProviderStateStore>) {
    let (dataset, selected) = seeded(store, false);
    let artifact = crate::compile(
        "IDENTIFICATION DIVISION. PROGRAM-ID. BOUNDED.
ENVIRONMENT DIVISION. INPUT-OUTPUT SECTION. FILE-CONTROL.
SELECT INPUT-FILE ASSIGN TO INPUTDD ORGANIZATION IS SEQUENTIAL.
SELECT OUTPUT-FILE ASSIGN TO OUTPUTDD ORGANIZATION IS SEQUENTIAL.
DATA DIVISION. FILE SECTION.
FD INPUT-FILE. 01 INPUT-RECORD PIC X(4).
FD OUTPUT-FILE. 01 OUTPUT-RECORD PIC X(4).
SD SORT-FILE. 01 SORT-RECORD. 05 SORT-KEY PIC X(2). 05 SORT-DATA PIC X(2).
PROCEDURE DIVISION.
SORT SORT-FILE ON ASCENDING KEY SORT-KEY USING INPUT-FILE GIVING OUTPUT-FILE.
DISPLAY 'UNREACHED'. STOP RUN.",
    )
    .unwrap();
    let mut invocation =
        pilot_invocation(&artifact, "IBMUSER", selected.as_str(), "BOUNDED").unwrap();
    // SORT uses this budget to bound its whole-input read. The provider has two records.
    invocation.limits.max_events = 1;
    for dd in ["INPUTDD", "OUTPUTDD"] {
        invocation.bindings.insert(
            format!("cobol.dd.{dd}"),
            argument(selected.as_str().as_bytes()),
        );
    }
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
                    panic!("expected SORT dataset read")
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
            MachineDrive::Failed(problem) => {
                assert_eq!(reads, 1);
                assert_eq!(problem.category, FailureCategory::ProviderFailure);
                assert_eq!(
                    problem.public_message,
                    format!(
                        "host provider failed: {:?}",
                        HostProblem::Condition {
                            name: "DATASET_READ_TRUNCATED".into(),
                            response: 16,
                            response2: 0,
                        }
                    )
                );
                assert!(!problem.retryable);
                assert!(!problem.unknown_outcome);
                break;
            }
            other => panic!("incomplete SORT input did not fail: {other:?}"),
        }
    }
}

#[test]
fn interpreter_sort_rejects_incomplete_whole_input() {
    interpreter_sort_case(memory_store());
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
fn interpreter_sort_rejects_incomplete_whole_input_postgres() {
    interpreter_sort_case(postgres_store());
}
