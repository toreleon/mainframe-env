use super::*;

mod secondary_tests;

fn two_pcb_catalog() -> ImsMetadataCatalog {
    let mut metadata = catalog();
    let mut hidden = metadata.databases[0].segments[1].clone();
    hidden.name = "HIDDEN".into();
    metadata.databases[0].segments.insert(1, hidden);
    let mut other = metadata.databases[0].clone();
    other.name = "OTHERDB".into();
    metadata.databases.push(other);
    let ImsPcbMetadata::Database(mut pcb) = metadata.psbs[0].pcbs[0].clone() else {
        panic!("database fixture");
    };
    pcb.name = "OTHERPCB".into();
    pcb.database = "OTHERDB".into();
    metadata.psbs[0].pcbs.push(ImsPcbMetadata::Database(pcb));
    metadata
}

fn routed(
    service: &Arc<ImsService>,
    run: &str,
    req: &ImsRequest,
) -> Result<ImsResult, HostProblem> {
    let invocation = invocation(run);
    let provider = ims_providers(service.clone(), InvocationLimits::default())
        .remove(usize::from(req.operation.is_mutating()));
    match provider
        .invoke(
            &invocation,
            EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence: req
                    .mutation
                    .as_ref()
                    .map_or(1, |mutation| mutation.sequence),
                deadline_tick: invocation.deadline_tick,
                idempotency_key: req
                    .mutation
                    .as_ref()
                    .map(|mutation| mutation.idempotency_key.clone()),
                request: HostRequest::Ims(req.clone()),
            },
        )
        .outcome?
    {
        HostResult::Ims(result) => Ok(result),
        _ => panic!("IMS route result"),
    }
}

fn call(service: &Arc<ImsService>, run: &str, req: &ImsRequest) -> ImsResult {
    routed(service, run, req).unwrap()
}

fn selected(
    run: &str,
    op: ImsOperation,
    sequence: u64,
    pcb: u16,
    segments: &[&str],
    data: &[u8],
) -> ImsRequest {
    let mut req = request(run, op, sequence, segments, data);
    req.pcb = pcb;
    req
}

fn seed(service: &Arc<ImsService>) {
    seed_catalog(service, two_pcb_catalog());
}

fn seed_catalog(service: &Arc<ImsService>, metadata: ImsMetadataCatalog) {
    service.install_metadata(metadata).unwrap();
    for (index, name, prefix) in [(0, "GENDB", b'A'), (1, "OTHERDB", b'B')] {
        let image = ImsGenericLoadImage {
            database: name.into(),
            records: vec![
                ImsGenericLoadRecord {
                    segment: "ROOT".into(),
                    parent: None,
                    data: vec![prefix, b'1', b'X'],
                },
                ImsGenericLoadRecord {
                    segment: "HIDDEN".into(),
                    parent: Some(0),
                    data: b"H1S".to_vec(),
                },
                ImsGenericLoadRecord {
                    segment: "CHILD".into(),
                    parent: Some(0),
                    data: b"C1V".to_vec(),
                },
                ImsGenericLoadRecord {
                    segment: "ROOT".into(),
                    parent: None,
                    data: vec![prefix, b'2', b'Y'],
                },
                ImsGenericLoadRecord {
                    segment: "HIDDEN".into(),
                    parent: Some(3),
                    data: b"H2S".to_vec(),
                },
            ],
        };
        let load = request(
            "pcb-seed",
            ImsOperation::Load,
            1 + index,
            &[],
            &serde_json::to_vec(&image).unwrap(),
        );
        assert_eq!(call(service, "pcb-seed", &load).affected_segments, 5);
    }
    call(
        service,
        "pcb-seed",
        &request("pcb-seed", ImsOperation::Commit, 3, &[], b""),
    );
}

fn exercise_selection(service: &Arc<ImsService>) {
    let run = "pcb-selection";
    call(
        service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    let left_hold = selected(run, ImsOperation::GetHoldUnique, 2, 1, &["ROOT"], b"");
    assert_eq!(call(service, run, &left_hold).segments[0].data, b"A1X");
    let left_position = service.lock().unwrap().state.sessions[run].position.clone();
    let right_hold = selected(run, ImsOperation::GetHoldUnique, 3, 2, &["ROOT"], b"");
    assert_eq!(call(service, run, &right_hold).segments[0].data, b"B1X");
    assert_eq!(
        service.lock().unwrap().state.sessions[run].position,
        left_position
    );
    assert_eq!(
        call(
            service,
            run,
            &selected(run, ImsOperation::Replace, 4, 1, &[], b"A1L")
        )
        .status,
        "  "
    );
    assert_eq!(
        call(
            service,
            run,
            &selected(run, ImsOperation::Replace, 5, 2, &[], b"B1R")
        )
        .status,
        "  "
    );
    let right_next = selected(run, ImsOperation::GetNext, 6, 2, &["ROOT"], b"");
    assert_eq!(call(service, run, &right_next).segments[0].data, b"B2Y");
    assert_eq!(call(service, run, &right_next).segments[0].data, b"B2Y");
    assert_eq!(call(service, run, &left_hold).segments[0].data, b"A1X");
    assert_eq!(
        call(
            service,
            run,
            &selected(run, ImsOperation::GetNext, 7, 1, &["ROOT"], b"")
        )
        .segments[0]
            .data,
        b"A2Y"
    );
}

#[test]
fn public_selected_pcb_positions_holds_replay_memory() {
    let service = ImsService::open(
        Arc::new(MemoryStore::new(Default::default())),
        Default::default(),
    )
    .unwrap();
    seed(&service);
    exercise_selection(&service);
}

#[test]
fn public_targetless_gn_skips_insensitive_segments() {
    let service = ImsService::open(
        Arc::new(MemoryStore::new(Default::default())),
        Default::default(),
    )
    .unwrap();
    seed(&service);
    let run = "pcb-gn";
    call(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    assert_eq!(
        call(
            &service,
            run,
            &request(run, ImsOperation::GetNext, 2, &[], b"")
        )
        .segments[0]
            .data,
        b"A1X"
    );
    let insensitive = request(run, ImsOperation::GetNext, 3, &["HIDDEN"], b"");
    let position = service.lock().unwrap().state.sessions[run].position.clone();
    assert_eq!(call(&service, run, &insensitive).status, "AC");
    assert_eq!(
        service.lock().unwrap().state.sessions[run].position,
        position
    );
    assert_eq!(
        call(
            &service,
            run,
            &request(run, ImsOperation::GetNext, 4, &[], b"")
        )
        .segments[0]
            .data,
        b"C1V"
    );
    assert_eq!(
        call(
            &service,
            run,
            &request(run, ImsOperation::GetNext, 5, &[], b"")
        )
        .segments[0]
            .data,
        b"A2Y"
    );
    let end = call(
        &service,
        run,
        &request(run, ImsOperation::GetNext, 6, &[], b""),
    );
    assert_eq!(end.status, "GB");
    assert!(end.segments.is_empty());
    assert_eq!(
        call(
            &service,
            run,
            &request(run, ImsOperation::GetNext, 7, &[], b"")
        )
        .segments[0]
            .data,
        b"A1X"
    );
}

#[test]
fn public_targetless_gnp_retains_visible_parentage_and_failure_position() {
    let service = ImsService::open(
        Arc::new(MemoryStore::new(Default::default())),
        Default::default(),
    )
    .unwrap();
    seed(&service);
    let run = "pcb-gnp";
    call(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    assert_eq!(
        call(
            &service,
            run,
            &request(run, ImsOperation::GetHoldUnique, 2, &["ROOT"], b"")
        )
        .segments[0]
            .data,
        b"A1X"
    );
    let parent = service.lock().unwrap().state.sessions[run]
        .position
        .parentage();
    let insensitive = request(run, ImsOperation::GetNextParent, 3, &["HIDDEN"], b"");
    assert_eq!(call(&service, run, &insensitive).status, "AC");
    assert_eq!(
        call(
            &service,
            run,
            &request(run, ImsOperation::GetHoldNextParent, 4, &[], b"")
        )
        .segments[0]
            .data,
        b"C1V"
    );
    let before = service.lock().unwrap().state.sessions[run].position.clone();
    assert_eq!(before.parentage(), parent);
    let end = call(
        &service,
        run,
        &request(run, ImsOperation::GetNextParent, 5, &[], b""),
    );
    assert_eq!(end.status, "GE");
    assert!(end.segments.is_empty());
    let after = service.lock().unwrap().state.sessions[run].position.clone();
    assert_eq!(after.current(), before.current());
    assert_eq!(after.parentage(), parent);
    assert!(!after.is_held());
}

fn exercise_reopen(
    store: Arc<dyn ProviderStateStore>,
    reopen: impl FnOnce() -> Arc<dyn ProviderStateStore>,
) {
    let service = ImsService::open(store, Default::default()).unwrap();
    seed(&service);
    exercise_selection(&service);
    let run = "pcb-selection";
    let scheduled = service.lock().unwrap().state.sessions[run].position.clone();
    let other = service.lock().unwrap().state.sessions[run].pcb_positions[&2].clone();
    assert_ne!(scheduled.current(), None);
    assert_ne!(other.current(), None);
    drop(service);
    let service = ImsService::open(reopen(), Default::default()).unwrap();
    let durable = service.lock().unwrap();
    assert_eq!(durable.state.sessions[run].pcb, 1);
    assert_eq!(durable.state.sessions[run].position, scheduled);
    assert_eq!(durable.state.sessions[run].pcb_positions[&2], other);
    drop(durable);
    let hold = selected(run, ImsOperation::GetHoldUnique, 20, 2, &["ROOT"], b"");
    assert_eq!(call(&service, run, &hold).segments[0].data, b"B1R");
    let held = service.lock().unwrap().state.sessions[run].pcb_positions[&2].clone();
    let mut miss = selected(run, ImsOperation::GetUnique, 21, 2, &["ROOT"], b"");
    miss.qualifiers.push(qualifier(b"ZZ"));
    let missing = call(&service, run, &miss);
    assert_eq!(missing.status, "GE");
    assert!(missing.segments.is_empty());
    let failed = service.lock().unwrap().state.sessions[run].pcb_positions[&2].clone();
    assert_eq!(failed.current(), held.current());
    assert_eq!(failed.parentage(), None);
    assert!(!failed.is_held());
    assert_eq!(
        service.lock().unwrap().state.sessions[run].position,
        scheduled
    );
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::GetNextParent, 22, 2, &[], b"")
        )
        .status,
        "GP"
    );
    assert_eq!(
        service.lock().unwrap().state.sessions[run].pcb_positions[&2],
        failed
    );
    call(
        &service,
        run,
        &request(run, ImsOperation::Rollback, 23, &[], b""),
    );
    let durable = service.lock().unwrap();
    assert_eq!(durable.state.sessions[run].position, PcbPosition::default());
    assert!(durable.state.sessions[run].pcb_positions.is_empty());
    drop(durable);
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::GetNext, 24, 2, &["ROOT"], b"")
        )
        .segments[0]
            .data,
        b"B1X"
    );
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::GetNext, 25, 1, &["ROOT"], b"")
        )
        .segments[0]
            .data,
        b"A1X"
    );
    let before = service.lock().unwrap().state.sessions[run].clone();
    let old = selected(run, ImsOperation::GetNext, 6, 2, &["ROOT"], b"");
    assert_eq!(call(&service, run, &old).segments[0].data, b"B2Y");
    assert_eq!(service.lock().unwrap().state.sessions[run], before);
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::GetHoldNextParent, 26, 2, &[], b"")
        )
        .segments[0]
            .data,
        b"C1V"
    );
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::GetNext, 27, 2, &[], b"")
        )
        .segments[0]
            .data,
        b"B2Y"
    );
    let before = service.lock().unwrap().state.sessions[run].pcb_positions[&2].clone();
    let exhausted = call(
        &service,
        run,
        &selected(run, ImsOperation::GetNextParent, 28, 2, &[], b""),
    );
    assert_eq!(exhausted.status, "GE");
    assert!(exhausted.segments.is_empty());
    assert_eq!(
        service.lock().unwrap().state.sessions[run].pcb_positions[&2],
        before
    );
}

#[test]
fn public_selected_pcb_memory_reopen_rollback_and_failure_positions() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let retained = store.clone();
    exercise_reopen(store, move || retained);
}

#[test]
fn public_selected_pcb_sqlite_fresh_connection_reopen_rollback_and_failure_positions() {
    let file = std::env::temp_dir().join(format!(
        "ims-pcb-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    exercise_reopen(store, || {
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap())
    });
    std::fs::remove_file(file).unwrap();
}

#[test]
fn public_restricted_procopt_and_key_only_reads_never_expose_data_or_mutate_database() {
    let service = ImsService::open(
        Arc::new(MemoryStore::new(Default::default())),
        Default::default(),
    )
    .unwrap();
    let mut metadata = two_pcb_catalog();
    let ImsPcbMetadata::Database(pcb) = &mut metadata.psbs[0].pcbs[1] else {
        panic!("DB PCB")
    };
    pcb.sensitive_segments[1].processing_options = Some("I".into());
    seed_catalog(&service, metadata);
    let run = "pcb-options";
    call(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    call(
        &service,
        run,
        &selected(run, ImsOperation::GetHoldUnique, 2, 2, &["ROOT"], b""),
    );
    let before = service.lock().unwrap().state.sessions[run].pcb_positions[&2].clone();
    let databases = service.lock().unwrap().state.generic_databases.clone();
    for (index, op) in [
        ImsOperation::GetNext,
        ImsOperation::GetNextParent,
        ImsOperation::GetHoldNext,
        ImsOperation::GetHoldNextParent,
    ]
    .into_iter()
    .enumerate()
    {
        let result = call(
            &service,
            run,
            &selected(run, op, 3 + index as u64, 2, &["CHILD"], b""),
        );
        assert_eq!(result.status, "AM");
        assert!(result.segments.is_empty());
        assert_eq!(
            service.lock().unwrap().state.sessions[run].pcb_positions[&2],
            before
        );
    }
    let result = call(
        &service,
        run,
        &selected(run, ImsOperation::Insert, 10, 2, &["HIDDEN"], b"H3T"),
    );
    assert_eq!(result.status, "AM");
    assert!(result.segments.is_empty());
    assert_eq!(result.affected_segments, 0);
    assert_eq!(service.lock().unwrap().state.generic_databases, databases);
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::GetNext, 11, 2, &[], b"")
        )
        .segments[0]
            .data,
        b"B2Y"
    );

    let service = ImsService::open(
        Arc::new(MemoryStore::new(Default::default())),
        Default::default(),
    )
    .unwrap();
    let mut metadata = two_pcb_catalog();
    let ImsPcbMetadata::Database(pcb) = &mut metadata.psbs[0].pcbs[1] else {
        panic!("DB PCB")
    };
    pcb.sensitive_segments[0].processing_options = Some("K".into());
    seed_catalog(&service, metadata);
    let run = "pcb-key-only";
    call(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    let root = call(
        &service,
        run,
        &selected(run, ImsOperation::GetUnique, 2, 2, &["ROOT"], b""),
    );
    assert_eq!(root.status, "  ");
    assert!(root.segments.is_empty());
    let denied = call(
        &service,
        run,
        &selected(run, ImsOperation::Replace, 3, 2, &[], b"B1S"),
    );
    assert_eq!(denied.status, "AM");
    assert_eq!(denied.affected_segments, 0);
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::GetNextParent, 4, 2, &[], b"")
        )
        .segments[0]
            .data,
        b"C1V"
    );
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::GetNext, 5, 2, &[], b"")
        )
        .status,
        "GB"
    );
}

#[derive(Default)]
struct SelectedPolicy {
    deny_other: Mutex<bool>,
    seen: Mutex<Vec<EnterpriseResource>>,
}

impl EnterpriseAuthorizer for SelectedPolicy {
    fn authorize(&self, _: &PrincipalId, resource: &EnterpriseResource) -> Result<(), HostProblem> {
        self.seen.lock().unwrap().push(resource.clone());
        if resource.class == EnterpriseResourceClass::ImsDatabase
            && resource.name.as_str() == "OTHERDB"
            && *self.deny_other.lock().unwrap()
        {
            Err(HostProblem::Unauthorized)
        } else {
            Ok(())
        }
    }
}

#[test]
fn selected_pcb_saf_precedes_observation_replay_holds_and_mutation() {
    let policy = Arc::new(SelectedPolicy::default());
    let service = ImsService::open_authorized(
        Arc::new(MemoryStore::new(Default::default())),
        Default::default(),
        policy.clone(),
    )
    .unwrap();
    seed(&service);
    let run = "pcb-saf";
    call(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    let mut read = selected(run, ImsOperation::GetHoldUnique, 2, 2, &["ROOT"], b"");
    read.q_class = Some(ImsQClass::new(b'A').unwrap());
    assert_eq!(call(&service, run, &read).segments[0].data, b"B1X");
    *policy.deny_other.lock().unwrap() = true;
    for req in [
        read.clone(),
        selected(run, ImsOperation::GetNextParent, 3, 2, &[], b""),
        selected(run, ImsOperation::Replace, 4, 2, &[], b"B1Z"),
    ] {
        policy.seen.lock().unwrap().clear();
        let before = service.lock().unwrap().state.clone();
        assert_eq!(routed(&service, run, &req), Err(HostProblem::Unauthorized));
        assert_eq!(service.lock().unwrap().state, before);
        let seen = policy.seen.lock().unwrap();
        assert!(
            seen.iter()
                .any(|item| item.class == EnterpriseResourceClass::ImsDatabase
                    && item.name.as_str() == "OTHERDB")
        );
        assert!(
            !seen
                .iter()
                .any(|item| item.class == EnterpriseResourceClass::ImsDatabase
                    && item.name.as_str() == "GENDB")
        );
    }
    *policy.deny_other.lock().unwrap() = false;
    let before = service.lock().unwrap().state.clone();
    assert_eq!(call(&service, run, &read).segments[0].data, b"B1X");
    assert_eq!(service.lock().unwrap().state, before);
}

#[test]
fn selected_pcb_status_and_q_reservations_use_independent_database_positions() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let service = ImsService::open(store.clone(), Default::default()).unwrap();
    seed(&service);
    let run = "pcb-q";
    call(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    for (sequence, pcb) in [(2, 1), (3, 2)] {
        let mut get = selected(
            run,
            ImsOperation::GetHoldUnique,
            sequence,
            pcb,
            &["ROOT"],
            b"",
        );
        get.q_class = Some(ImsQClass::new(b'A').unwrap());
        call(&service, run, &get);
    }
    let row = store
        .get_provider_state(SYSTEM_NAMESPACE, "runtime")
        .unwrap()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
    let reservations = value["value"]["reservations"].as_object().unwrap();
    assert_eq!(reservations.len(), 2);
    assert!(reservations.keys().any(|key| key.starts_with("GENDB:")));
    assert!(reservations.keys().any(|key| key.starts_with("OTHERDB:")));
    assert!(reservations.values().all(|item| item["current"] == true));
    call(
        &service,
        run,
        &selected(run, ImsOperation::Replace, 4, 2, &[], b"B1Z"),
    );
    let row = store
        .get_provider_state(SYSTEM_NAMESPACE, "runtime")
        .unwrap()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
    for (key, reservation) in value["value"]["reservations"].as_object().unwrap() {
        assert_eq!(reservation["modified"], key.starts_with("OTHERDB:"));
    }
    let result = call(
        &service,
        run,
        &selected(run, ImsOperation::GetNext, 5, 2, &["HIDDEN"], b""),
    );
    assert_eq!(result.status, "AC");
    assert!(result.segments.is_empty());
    let refresh = system_request(
        run,
        6,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Command,
        ImsSystemCall::Refresh,
    );
    let result = call(&service, run, &refresh);
    let Some(ImsSystemResult::Refreshed { pcbs }) = result.system else {
        panic!("REFRESH")
    };
    assert_eq!(pcbs[0].status, "  ");
    assert_eq!(pcbs[1].status, "AC");
    call(
        &service,
        run,
        &request(run, ImsOperation::Rollback, 7, &[], b""),
    );
    assert_eq!(system::reservation_count(&service.lock().unwrap().state), 0);
}

#[test]
fn invalid_and_non_database_pcb_reject_without_observation_or_row_changes() {
    let service = ImsService::open(
        Arc::new(MemoryStore::new(Default::default())),
        Default::default(),
    )
    .unwrap();
    seed(&service);
    let run = "pcb-invalid";
    call(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    for (sequence, number, expected) in [
        (2, 0, HostProblem::Malformed),
        (3, 3, HostProblem::NotFound),
    ] {
        let before = service.lock().unwrap().state.clone();
        assert_eq!(
            routed(
                &service,
                run,
                &selected(run, ImsOperation::GetNext, sequence, number, &[], b"")
            ),
            Err(expected)
        );
        assert_eq!(service.lock().unwrap().state, before);
    }
    let before = service.lock().unwrap().state.clone();
    let mut changed = before.clone();
    // The typed metadata's existing alternate PCB cannot become a DB call route.
    changed.metadata.as_mut().unwrap().psbs[0].pcbs[1] =
        ImsPcbMetadata::AlternateTerminal(mainframe_env_host_api::ImsTerminalPcbMetadata {
            name: "ALTPCB".into(),
            modifiable: true,
            express: false,
            destination: None,
            same_terminal: false,
            response_mode: false,
        });
    // Use the typed installer in the separate non-DB fixture below, not a live mutation.
    let metadata = changed.metadata.unwrap();
    let other = ImsService::open(
        Arc::new(MemoryStore::new(Default::default())),
        Default::default(),
    )
    .unwrap();
    other.install_metadata(metadata).unwrap();
    call(
        &other,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    let before = other.lock().unwrap().state.clone();
    assert_eq!(
        routed(
            &other,
            run,
            &selected(run, ImsOperation::GetNext, 2, 2, &[], b"")
        ),
        Err(HostProblem::Unsupported)
    );
    assert_eq!(other.lock().unwrap().state, before);
}

mod reader_tests;

#[test]
fn gnp_parent_qualification_failure_is_ge_and_target_at_parent_is_gp_without_position_change() {
    let service = ImsService::open(
        Arc::new(MemoryStore::new(Default::default())),
        Default::default(),
    )
    .unwrap();
    seed(&service);
    let run = "pcb-parent-failure";
    call(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    call(
        &service,
        run,
        &selected(run, ImsOperation::GetHoldUnique, 2, 2, &["ROOT"], b""),
    );
    let before = service.lock().unwrap().state.sessions[run].pcb_positions[&2].clone();
    let mut mismatch = selected(run, ImsOperation::GetNextParent, 3, 2, &["CHILD"], b"");
    mismatch.qualifiers.push(qualifier(b"ZZ"));
    assert_eq!(call(&service, run, &mismatch).status, "GE");
    assert_eq!(
        service.lock().unwrap().state.sessions[run].pcb_positions[&2],
        before
    );
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::GetNextParent, 4, 2, &["ROOT"], b"")
        )
        .status,
        "GP"
    );
    assert_eq!(
        service.lock().unwrap().state.sessions[run].pcb_positions[&2],
        before
    );
}

#[test]
fn two_pcbs_on_same_database_keep_holds_independent_and_delete_invalidates_only_removed_positions()
{
    let service = ImsService::open(
        Arc::new(MemoryStore::new(Default::default())),
        Default::default(),
    )
    .unwrap();
    let mut metadata = two_pcb_catalog();
    let ImsPcbMetadata::Database(mut pcb) = metadata.psbs[0].pcbs[0].clone() else {
        panic!("DB PCB")
    };
    pcb.name = "THIRDPCB".into();
    metadata.psbs[0].pcbs.push(ImsPcbMetadata::Database(pcb));
    seed_catalog(&service, metadata);
    let run = "pcb-same-db";
    call(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    call(
        &service,
        run,
        &selected(run, ImsOperation::GetHoldUnique, 2, 1, &["ROOT"], b""),
    );
    let mut third = selected(run, ImsOperation::GetHoldUnique, 3, 3, &["ROOT"], b"");
    third.qualifiers.push(qualifier(b"A2"));
    call(&service, run, &third);
    let independent = service.lock().unwrap().state.sessions[run].pcb_positions[&3].clone();
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::Replace, 4, 1, &[], b"A1L")
        )
        .status,
        "  "
    );
    assert_eq!(
        service.lock().unwrap().state.sessions[run].pcb_positions[&3],
        independent
    );
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::Delete, 5, 1, &[], b"")
        )
        .status,
        "  "
    );
    assert_eq!(
        service.lock().unwrap().state.sessions[run].pcb_positions[&3],
        independent
    );
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::Replace, 6, 3, &[], b"A2Z")
        )
        .status,
        "  "
    );
    call(
        &service,
        run,
        &selected(run, ImsOperation::GetHoldUnique, 7, 1, &["ROOT"], b""),
    );
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::Delete, 8, 3, &[], b"")
        )
        .status,
        "  "
    );
    assert_eq!(
        service.lock().unwrap().state.sessions[run].position,
        PcbPosition::default()
    );
}
