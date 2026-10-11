use super::*;

fn runtime(store: Arc<SqliteStateStore>) -> Arc<CicsService> {
    CicsService::open_with_runtime(
        authorities(),
        store.clone(),
        store,
        CicsLimits::default(),
        Arc::new(TestCicsClock::fixed(1_000)),
    )
    .unwrap()
}

fn database() -> (std::path::PathBuf, Arc<SqliteStateStore>) {
    let root = std::env::temp_dir().join(format!(
        "retrieve-compatibility-197-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let store = open_database(&root);
    (root, store)
}

fn open_database(root: &std::path::Path) -> Arc<SqliteStateStore> {
    Arc::new(
        SqliteStateStore::open(
            &format!("sqlite://{}?mode=rwc", root.join("state.db").display()),
            8 * 1024 * 1024,
            65_536,
        )
        .unwrap(),
    )
}

fn consumer(service: &CicsService, name: &str) -> Invocation {
    let invocation = invocation_for(
        name,
        BTreeMap::from([("cics.retrieve".into(), argument(b"CONTINUATION"))]),
    );
    let session = SessionId::new(format!("{name}-session"), 64).unwrap();
    if !service
        .lock()
        .unwrap()
        .sessions
        .contains_key(session.as_str())
    {
        service.create_session(&session, 24, 80).unwrap();
    }
    service
        .register_run(invocation.clone(), &session, "MENU", "MEAPPL", "MESYS")
        .unwrap();
    invocation
}

fn ready_start(service: &CicsService, store: &dyn WorkStore, terminal: Option<&str>) {
    let issuer = registered_counter_run(service, "issuer");
    let mut arguments = BTreeMap::from([
        ("TRANSID".into(), argument(b"MENU")),
        ("REQID".into(), argument(b"START197")),
        ("FROM".into(), argument(b"START-DATA")),
    ]);
    if let Some(terminal) = terminal {
        arguments.insert("TERMID".into(), argument(terminal.as_bytes()));
    }
    let start = request(CicsOperation::Start, arguments, 10);
    service
        .invoke(&effect(&issuer.run_unit_id, start.clone(), 10), start)
        .unwrap();
    let work = store
        .claim("worker", Some(CICS_START_WORK_GENERATION), 1_000, 100)
        .unwrap()
        .unwrap();
    service.promote_start_work(&work, 1_000).unwrap();
    store
        .complete(
            &work.work_id,
            work.lease_id.as_deref().unwrap(),
            work.lease_epoch,
            1_000,
        )
        .unwrap();
}

fn retrieve(sequence: u64) -> CicsRequest {
    let mut request = request(CicsOperation::Retrieve, BTreeMap::new(), sequence);
    request.condition_policy = CicsConditionPolicy::Respond {
        response_field: "RESP".into(),
        response2_field: Some("RESP2".into()),
    };
    request
}

fn invoke(service: &CicsService, invocation: &Invocation, sequence: u64) -> CicsResponse {
    let request = retrieve(sequence);
    service
        .invoke(
            &effect(&invocation.run_unit_id, request.clone(), sequence),
            request,
        )
        .unwrap()
}

fn assert_enddata(response: &CicsResponse) {
    assert_eq!(
        (
            response.condition.as_str(),
            response.response,
            response.response2
        ),
        ("ENDDATA", 29, 0),
        "RETRIEVE must not return continuation COMMAREA: {:?}",
        response.payload.bytes()
    );
    assert!(response.payload.bytes().is_empty());
}

#[test]
fn no_start_record_returns_enddata_instead_of_continuation_commarea() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let service = CicsService::open_with_runtime(
        authorities(),
        store.clone(),
        store,
        CicsLimits::default(),
        Arc::new(TestCicsClock::fixed(1_000)),
    )
    .unwrap();
    let consumer = consumer(&service, "consumer");
    assert_enddata(&invoke(&service, &consumer, 1));
}

#[test]
fn one_start_record_returns_start_data_instead_of_continuation_commarea() {
    let (root, store) = database();
    let service = runtime(store.clone());
    ready_start(&service, store.as_ref(), None);
    let consumer = consumer(&service, "consumer");
    let response = invoke(&service, &consumer, 1);
    assert_eq!(response.condition, "NORMAL");
    assert_eq!(response.payload.bytes(), b"START-DATA");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn repeated_retrieve_consumes_start_record_only_once() {
    let (root, store) = database();
    let service = runtime(store.clone());
    ready_start(&service, store.as_ref(), None);
    let consumer = consumer(&service, "consumer");
    invoke(&service, &consumer, 1);
    assert_enddata(&invoke(&service, &consumer, 2));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn crash_after_start_consumption_replays_without_losing_or_duplicating_data() {
    let (root, store) = database();
    let service = runtime(store.clone());
    ready_start(&service, store.as_ref(), None);
    let invocation = consumer(&service, "consumer");
    let mut run = service.lock().unwrap().runs[&invocation.run_unit_id].clone();
    // Execute the lowest-layer handler, then discard the provider before the
    // outer effect receipt is written: consumption must carry its own replay identity.
    let response = handlers::invoke_interval_control(&service, &mut run, &retrieve(1)).unwrap();
    drop(service);
    drop(store);
    let reopened = runtime(open_database(&root));
    let invocation = consumer(&reopened, "consumer");
    assert_eq!(invoke(&reopened, &invocation, 1), response);
    assert_enddata(&invoke(&reopened, &invocation, 2));
    assert_eq!(response.payload.bytes(), b"START-DATA");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn concurrent_consumers_return_start_record_to_only_one_effect() {
    let (root, store) = database();
    let first = runtime(store.clone());
    ready_start(&first, store.as_ref(), None);
    let second = runtime(open_database(&root));
    let first_run = consumer(&first, "first");
    let second_run = consumer(&second, "second");
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let responses = std::thread::scope(|scope| {
        let spawn = |service: Arc<CicsService>, invocation: Invocation, sequence| {
            let barrier = barrier.clone();
            scope.spawn(move || {
                let mut run = service.lock().unwrap().runs[&invocation.run_unit_id].clone();
                barrier.wait();
                handlers::invoke_interval_control(&service, &mut run, &retrieve(sequence))
            })
        };
        let a = spawn(first, first_run, 1);
        let b = spawn(second, second_run, 2);
        [a.join().unwrap(), b.join().unwrap()]
    });
    assert_eq!(
        responses
            .iter()
            .filter(|result| result
                .as_ref()
                .is_ok_and(|response| response.payload.bytes() == b"START-DATA"))
            .count(),
        1
    );
    assert_eq!(
        responses
            .iter()
            .filter_map(|result| result.as_ref().err())
            .collect::<Vec<_>>(),
        [&HostProblem::IdempotencyConflict]
    );
    // A CAS loser retries from recovered durable state, never from its stale snapshot.
    let reopened = runtime(open_database(&root));
    let consumer = consumer(&reopened, "retry");
    assert_enddata(&invoke(&reopened, &consumer, 3));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn pseudo_conversation_commarea_survives_start_retrieve_and_enddata() {
    let (root, store) = database();
    let service = runtime(store.clone());
    let initial = invocation_for("pseudo-initial", BTreeMap::new());
    let session = SessionId::new("pseudo-session", 64).unwrap();
    service
        .launch_terminal(
            initial.clone(),
            &session,
            "MENU",
            24,
            80,
            "csrf-197",
            1_000,
            10_000,
        )
        .unwrap();
    let terminal = service.lock().unwrap().sessions[session.as_str()]
        .input
        .terminal_id
        .clone()
        .unwrap();
    ready_start(&service, store.as_ref(), Some(&terminal));
    let returned = request(
        CicsOperation::Return,
        BTreeMap::from([
            ("TRANSID".into(), argument(b"MENU")),
            ("COMMAREA".into(), argument(b"CONTINUATION")),
        ]),
        11,
    );
    service
        .invoke(
            &effect(&initial.run_unit_id, returned.clone(), 11),
            returned,
        )
        .unwrap();
    let resumed = invocation_for("resumed", BTreeMap::new());
    service
        .resume_terminal(resumed.clone(), &session, "csrf-197", 1_001)
        .unwrap();
    let entry = service
        .terminal_execution(&session, resumed.principal.id(), 1_001)
        .unwrap();
    assert_eq!(entry.commarea, b"CONTINUATION");
    // This public entry contract supplies DFHCOMMAREA and its EIBCALEN length.
    assert_eq!(entry.commarea.len(), 12);
    assert_eq!(invoke(&service, &resumed, 1).payload.bytes(), b"START-DATA");
    assert_enddata(&invoke(&service, &resumed, 2));
    assert_eq!(
        service
            .terminal_execution(&session, resumed.principal.id(), 1_001)
            .unwrap()
            .commarea,
        entry.commarea
    );
    std::fs::remove_dir_all(root).unwrap();
}
