use super::*;

fn indexed_request(run: &str, operation: ImsOperation, sequence: u64, value: &[u8]) -> ImsRequest {
    let mut req = selected(run, operation, sequence, 2, &["ROOT"], b"");
    req.qualifiers = vec![ImsQualifier {
        segment: "ROOT".into(),
        field: "BYCHILD".into(),
        value: value.into(),
    }];
    req
}

fn seed_reopen(store: Arc<dyn ProviderStateStore>) -> (ImsRequest, ImsResult) {
    let service = ImsService::open(store.clone(), Default::default()).unwrap();
    seed_index(&service, true, true);
    let run = "index-reopen";
    call(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    let get = indexed_request(run, ImsOperation::GetHoldUnique, 2, b"AZ");
    let expected = call(&service, run, &get);
    assert_eq!(expected.segments[0].data, b"A1ZX");
    // GNP remains in the pointer's target subtree and retains target parentage.
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::GetHoldNextParent, 3, 2, &["CHILD"], b"")
        )
        .segments[0]
            .data,
        b"C1ZA"
    );
    let mut position = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    assert_ne!(position.current(), position.parentage());
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::Replace, 4, 2, &[], b"C1BY")
        )
        .status,
        "  "
    );
    position = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    assert_eq!(position.parentage(), None);
    assert!(position.is_held());
    assert!(lookup(&service, b"AZ").is_empty());
    assert_eq!(lookup(&service, b"YB"), vec![b"A1ZX".to_vec()]);
    call(
        &service,
        run,
        &request(run, ImsOperation::Commit, 5, &[], b""),
    );
    // Reestablish and retain the selected pointer cursor for a fresh reader.
    assert_eq!(
        call(
            &service,
            run,
            &indexed_request(run, ImsOperation::GetUnique, 6, b"YB")
        )
        .segments[0]
            .data,
        b"A1ZX"
    );
    (get, expected)
}

fn verify_reopen(store: Arc<dyn ProviderStateStore>, get: &ImsRequest, expected: &ImsResult) {
    let run = "index-reopen";
    let raced_store = super::session_cas::SessionCasStore::new(store.clone(), run);
    let service = ImsService::open(raced_store.clone(), Default::default()).unwrap();
    assert_eq!(lookup(&service, b"YB"), vec![b"A1ZX".to_vec()]);
    let cursor = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    assert_eq!(&call(&service, run, get), expected);
    assert_eq!(
        pcb::position(&service.lock().unwrap().state.sessions[run], 2),
        cursor
    );
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::GetNext, 7, 2, &["ROOT"], b"")
        )
        .segments[0]
            .data,
        b"B2AX"
    );
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::GetNext, 8, 2, &["ROOT"], b"")
        )
        .status,
        "GE"
    );
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::GetNext, 9, 2, &[], b"")
        )
        .segments[0]
            .data,
        b"A1ZX"
    );
    // Target deletion and insertion through PROCSEQ are forbidden even with AP.
    call(
        &service,
        run,
        &indexed_request(run, ImsOperation::GetHoldUnique, 10, b"YB"),
    );
    let before = restored(&service.lock().unwrap().state, "GENDB", Default::default())
        .unwrap()
        .state_digest();
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::Delete, 11, 2, &[], b"")
        )
        .status,
        "AM"
    );
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::Insert, 12, 2, &["ROOT"], b"D4XY")
        )
        .status,
        "AM"
    );
    assert_eq!(
        restored(&service.lock().unwrap().state, "GENDB", Default::default())
            .unwrap()
            .state_digest(),
        before
    );
    // An independently advanced session CAS must reject the entire replacement.
    let mut child = selected(run, ImsOperation::GetHoldUnique, 13, 1, &["CHILD"], b"");
    child.qualifiers = vec![qualifier(b"A1")];
    call(&service, run, &child);
    let prior_row = store
        .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
        .unwrap()
        .unwrap();
    raced_store.arm();
    let replacement = request(run, ImsOperation::Replace, 14, &[], b"C1DX");
    assert_eq!(
        routed(&service, run, &replacement),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(
        store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
            .unwrap()
            .unwrap(),
        prior_row
    );
    assert_eq!(lookup(&service, b"YB"), vec![b"A1ZX".to_vec()]);
    drop(service);
    let service = ImsService::open(store, Default::default()).unwrap();
    assert_eq!(call(&service, run, &replacement).status, "  ");
    assert_eq!(lookup(&service, b"XD"), vec![b"A1ZX".to_vec()]);
    call(
        &service,
        run,
        &request(run, ImsOperation::Rollback, 15, &[], b""),
    );
    assert!(lookup(&service, b"XD").is_empty());
    assert_eq!(lookup(&service, b"YB"), vec![b"A1ZX".to_vec()]);
    call(
        &service,
        run,
        &indexed_request(run, ImsOperation::GetUnique, 16, b"YB"),
    );
    let parentage = pcb::position(&service.lock().unwrap().state.sessions[run], 2).parentage();
    let mut insert = selected(run, ImsOperation::Insert, 17, 2, &["CHILD"], b"C3EX");
    insert.qualifiers = vec![ImsQualifier {
        segment: "ROOT".into(),
        field: "BYCHILD".into(),
        value: b"YB".into(),
    }];
    assert_eq!(call(&service, run, &insert).status, "  ");
    assert_eq!(
        pcb::position(&service.lock().unwrap().state.sessions[run], 2).parentage(),
        parentage
    );
    assert_eq!(lookup(&service, b"XE"), vec![b"A1ZX".to_vec()]);
    let mut held = selected(run, ImsOperation::GetHoldUnique, 18, 2, &["CHILD"], b"");
    held.qualifiers = vec![
        ImsQualifier {
            segment: "ROOT".into(),
            field: "BYCHILD".into(),
            value: b"XE".into(),
        },
        ImsQualifier {
            segment: "CHILD".into(),
            field: "CHILDKEY".into(),
            value: b"C3".into(),
        },
    ];
    assert_eq!(call(&service, run, &held).segments[0].data, b"C3EX");
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::Delete, 19, 2, &[], b"")
        )
        .status,
        "  "
    );
    assert!(lookup(&service, b"XE").is_empty());
    call(
        &service,
        run,
        &request(run, ImsOperation::Rollback, 20, &[], b""),
    );
}

#[test]
fn secondary_memory_reopen_replay_rollback_and_atomic_cas() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let (get, result) = seed_reopen(store.clone());
    verify_reopen(store, &get, &result);
}

#[test]
fn secondary_corrupt_pointer_binding_rejected_on_fresh_reader() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    seed_reopen(store.clone());
    let mut row = store
        .get_provider_state(SESSION_NAMESPACE, "index-reopen")
        .unwrap()
        .unwrap();
    let mut payload: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
    payload["value"]["pcb_positions"]["2"]["secondary"]["index"] = serde_json::json!("BYROOT");
    row.payload = serde_json::to_vec(&payload).unwrap();
    let prior = row.version;
    row.version += 1;
    store.put_provider_state(row, Some(prior)).unwrap();
    assert!(matches!(
        ImsService::open(store, Default::default()),
        Err(HostProblem::InfrastructureFailure)
    ));
}

#[test]
fn secondary_sqlite_fresh_connections_replay_rollback_and_atomic_cas() {
    let file = std::env::temp_dir().join(format!(
        "ims-index-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    let (get, result) = seed_reopen(Arc::new(
        SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap(),
    ));
    verify_reopen(
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap()),
        &get,
        &result,
    );
    std::fs::remove_file(file).unwrap();
}

#[test]
fn secondary_unavailable_metadata_and_authorization_do_not_mutate() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let policy = Arc::new(Policy::default());
    let service =
        ImsService::open_authorized(store.clone(), Default::default(), policy.clone()).unwrap();
    for shape in 0..4 {
        let mut metadata = indexed_catalog(true, true);
        let expected = if shape == 1 {
            HostProblem::Unsupported
        } else {
            HostProblem::Malformed
        };
        match shape {
            0 => {
                metadata.databases[0].secondary_indexes[0].target_segment = "CHILD".into();
                let ImsPcbMetadata::Database(pcb) = &mut metadata.psbs[0].pcbs[1] else {
                    unreachable!()
                };
                pcb.sensitive_segments.remove(0);
            }
            1 => metadata.databases[0].organization = ImsDatabaseOrganization::Dedb,
            2 => metadata.databases[0].secondary_indexes[0].source_segment = "ROOT".into(),
            _ => {
                let ImsPcbMetadata::Database(pcb) = &mut metadata.psbs[0].pcbs[1] else {
                    unreachable!()
                };
                pcb.secondary_index = Some("MISSING".into());
            }
        }
        if shape == 2 {
            metadata.databases[0].secondary_indexes[0].target_segment = "CHILD".into();
        }
        let before = service.lock().unwrap().state.clone();
        assert_eq!(service.install_metadata(metadata), Err(expected));
        assert_eq!(service.lock().unwrap().state, before);
    }
    seed_index(&service, true, true);
    let run = "index-auth";
    call(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    let read = indexed_request(run, ImsOperation::GetUnique, 2, b"AZ");
    let before = service.lock().unwrap().state.clone();
    // Positioned Gets mutate the retained PCB and use the accepted Update intent.
    *policy.deny_update.lock().unwrap() = true;
    assert_eq!(routed(&service, run, &read), Err(HostProblem::Unauthorized));
    assert_eq!(service.lock().unwrap().state, before);
    *policy.deny_update.lock().unwrap() = false;
    call(&service, run, &read);
    call(
        &service,
        run,
        &indexed_request(run, ImsOperation::GetHoldUnique, 3, b"AZ"),
    );
    let replacement = selected(run, ImsOperation::Replace, 4, 2, &[], b"A1WX");
    let before = service.lock().unwrap().state.clone();
    *policy.deny_update.lock().unwrap() = true;
    assert_eq!(
        routed(&service, run, &replacement),
        Err(HostProblem::Unauthorized)
    );
    assert_eq!(service.lock().unwrap().state, before);
    *policy.deny_update.lock().unwrap() = false;
    assert_eq!(call(&service, run, &replacement).status, "  ");
    // This selected XDFLD comes from CHILD, not the replaced ROOT bytes.
    assert_eq!(
        pcb::position(&service.lock().unwrap().state.sessions[run], 2).parentage(),
        pcb::position(&before.sessions[run], 2).parentage()
    );
    *policy.deny_update.lock().unwrap() = true;
    assert_eq!(
        routed(&service, run, &replacement),
        Err(HostProblem::Unauthorized)
    );
}
