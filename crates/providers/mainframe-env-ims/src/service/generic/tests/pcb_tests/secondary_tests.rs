use super::*;

mod recovery_tests;
mod session_cas;

#[test]
fn secondary_rich_ssa_never_silently_uses_primary_order() {
    let service = ImsService::open(
        Arc::new(MemoryStore::new(Default::default())),
        Default::default(),
    )
    .unwrap();
    seed_index(&service, false, true);
    let run = "secondary-ssa-boundary";
    call(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    let invocation = invocation(run);
    let req = selected(run, ImsOperation::GetNext, 2, 2, &[], b"");
    let effect = EffectRequest {
        run_unit: invocation.run_unit_id.clone(),
        sequence: 2,
        deadline_tick: invocation.deadline_tick,
        idempotency_key: req.mutation.as_ref().map(|m| m.idempotency_key.clone()),
        request: HostRequest::ImsNavigation(mainframe_env_host_api::ImsNavigationRequest {
            request: req,
            context: mainframe_env_host_api::ImsExecutionContext::DbBatch,
            ssas: vec![b"ROOT     ".to_vec()],
        }),
    };
    let before = serde_json::to_vec(&service.lock().unwrap().state).unwrap();
    assert_eq!(
        ims_providers(service.clone(), InvocationLimits::default())[1]
            .invoke(&invocation, effect)
            .outcome,
        Err(HostProblem::Unsupported)
    );
    assert_eq!(
        serde_json::to_vec(&service.lock().unwrap().state).unwrap(),
        before
    );
}

fn indexed_catalog(composite: bool, selected_index: bool) -> ImsMetadataCatalog {
    let mut metadata = catalog();
    for segment in &mut metadata.databases[0].segments {
        segment.max_length = 4;
        segment.min_length = 4;
        segment.fields.push(ImsFieldMetadata {
            name: Some("ZONE".into()),
            offset: 3,
            length: 1,
            sequence: false,
            unique: false,
        });
    }
    metadata.databases[0].secondary_indexes = vec![ImsSecondaryIndexMetadata {
        name: "BYCHILD".into(),
        source_segment: if composite { "CHILD" } else { "ROOT" }.into(),
        target_segment: "ROOT".into(),
        source_fields: if composite {
            vec!["ZONE".into(), "KIND".into()]
        } else {
            vec!["KIND".into()]
        },
    }];
    if selected_index {
        let mut pcb = serde_json::to_value(&metadata.psbs[0].pcbs[0]).unwrap();
        pcb["name"] = serde_json::json!("INDEXPCB");
        pcb["secondary_index"] = serde_json::json!("BYCHILD");
        metadata.psbs[0]
            .pcbs
            .push(serde_json::from_value(pcb).expect("secondary PCB selector"));
        metadata.databases[0]
            .secondary_indexes
            .push(ImsSecondaryIndexMetadata {
                name: "BYROOT".into(),
                source_segment: "ROOT".into(),
                target_segment: "ROOT".into(),
                source_fields: vec!["KIND".into()],
            });
        let mut pcb = serde_json::to_value(&metadata.psbs[0].pcbs[1]).unwrap();
        pcb["name"] = serde_json::json!("ROOTPCB");
        pcb["secondary_index"] = serde_json::json!("BYROOT");
        metadata.psbs[0]
            .pcbs
            .push(serde_json::from_value(pcb).unwrap());
    }
    metadata
}

fn seed_index(service: &Arc<ImsService>, composite: bool, selected_index: bool) {
    service
        .install_metadata(indexed_catalog(composite, selected_index))
        .unwrap();
    let records = [
        ("ROOT", None, b"A1ZX"),
        ("CHILD", Some(0), b"C1ZA"),
        ("ROOT", None, b"B2AX"),
        ("CHILD", Some(2), b"C2AZ"),
    ]
    .into_iter()
    .map(|(segment, parent, data)| ImsGenericLoadRecord {
        segment: segment.into(),
        parent,
        data: data.to_vec(),
    })
    .collect();
    let image = ImsGenericLoadImage {
        database: "GENDB".into(),
        records,
    };
    call(
        service,
        "index-seed",
        &request(
            "index-seed",
            ImsOperation::Load,
            1,
            &[],
            &serde_json::to_vec(&image).unwrap(),
        ),
    );
    call(
        service,
        "index-seed",
        &request("index-seed", ImsOperation::Commit, 2, &[], b""),
    );
}

fn lookup(service: &Arc<ImsService>, value: &[u8]) -> Vec<Vec<u8>> {
    restored(
        &service.lock().unwrap().state,
        "GENDB",
        ImsLimits::default(),
    )
    .unwrap()
    .lookup_index("BYCHILD", value)
    .unwrap()
    .into_iter()
    .map(|v| v.data)
    .collect()
}

#[test]
fn secondary_composite_bytes_target_ancestor_maintenance_public_provider() {
    let service = ImsService::open(
        Arc::new(MemoryStore::new(Default::default())),
        Default::default(),
    )
    .unwrap();
    exercise_maintenance(&service);
}

fn exercise_maintenance(service: &Arc<ImsService>) {
    seed_index(service, true, false);
    assert_eq!(lookup(service, b"AZ"), vec![b"A1ZX".to_vec()]);
    assert_eq!(lookup(service, b"ZA"), vec![b"B2AX".to_vec()]);
    let run = "index-maintenance";
    call(
        service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    let mut child = request(run, ImsOperation::GetHoldUnique, 2, &["CHILD"], b"");
    child.qualifiers = vec![qualifier(b"A1")];
    assert_eq!(call(service, run, &child).segments[0].data, b"C1ZA");
    let replace = request(run, ImsOperation::Replace, 3, &[], b"C1BY");
    assert_eq!(call(service, run, &replace).status, "  ");
    assert!(lookup(service, b"AZ").is_empty());
    assert_eq!(lookup(service, b"YB"), vec![b"A1ZX".to_vec()]);
    assert_eq!(call(service, run, &replace).status, "  ");
    call(
        service,
        run,
        &request(run, ImsOperation::Delete, 4, &[], b""),
    );
    assert!(lookup(service, b"YB").is_empty());
    let mut insert = request(run, ImsOperation::Insert, 5, &["CHILD"], b"C3DX");
    insert.qualifiers = vec![qualifier(b"A1")];
    assert_eq!(call(service, run, &insert).status, "  ");
    assert_eq!(lookup(service, b"XD"), vec![b"A1ZX".to_vec()]);
    call(
        service,
        run,
        &request(run, ImsOperation::Rollback, 6, &[], b""),
    );
    assert_eq!(lookup(service, b"AZ"), vec![b"A1ZX".to_vec()]);
    assert!(lookup(service, b"XD").is_empty());
    let mut root = request(run, ImsOperation::GetHoldUnique, 7, &["ROOT"], b"");
    root.qualifiers = vec![qualifier(b"A1")];
    call(service, run, &root);
    assert_eq!(
        call(
            service,
            run,
            &request(run, ImsOperation::Delete, 8, &[], b"")
        )
        .affected_segments,
        2
    );
    assert!(lookup(service, b"AZ").is_empty());
    assert_eq!(lookup(service, b"ZA"), vec![b"B2AX".to_vec()]);
    call(
        service,
        run,
        &request(run, ImsOperation::Rollback, 9, &[], b""),
    );
    assert_eq!(lookup(service, b"AZ"), vec![b"A1ZX".to_vec()]);
}

fn exercise_secondary_positions(service: &Arc<ImsService>, composite: bool) {
    seed_index(service, composite, true);
    let run = "index-navigation";
    call(
        service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    let primary = selected(run, ImsOperation::GetHoldUnique, 2, 1, &["ROOT"], b"");
    assert_eq!(call(service, run, &primary).segments[0].data, b"A1ZX");
    let prior = service.lock().unwrap().state.sessions[run].position.clone();
    let next = selected(run, ImsOperation::GetNext, 3, 2, &["ROOT"], b"");
    let expected = if composite { b"A1ZX" } else { b"B2AX" };
    assert_eq!(call(service, run, &next).segments[0].data, expected);
    assert_eq!(call(service, run, &next).segments[0].data, expected);
    assert_eq!(service.lock().unwrap().state.sessions[run].position, prior);
    let mut missing = selected(run, ImsOperation::GetUnique, 4, 2, &["ROOT"], b"");
    missing.qualifiers = vec![ImsQualifier {
        segment: "ROOT".into(),
        field: "BYCHILD".into(),
        value: b"??".to_vec(),
    }];
    let index_prior = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    assert_eq!(call(service, run, &missing).status, "GE");
    let failed = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    assert_eq!(failed.current(), index_prior.current());
    assert_eq!(failed.parentage(), None);
    assert!(!failed.is_held());
    assert_eq!(
        call(
            service,
            run,
            &selected(run, ImsOperation::GetNextParent, 5, 2, &["CHILD"], b"")
        )
        .status,
        "GP"
    );
    let mut qualified = missing.clone();
    qualified.mutation = request(run, ImsOperation::GetUnique, 6, &[], b"").mutation;
    qualified.qualifiers[0].value = if composite {
        b"ZA".to_vec()
    } else {
        b"A".to_vec()
    };
    assert_eq!(call(service, run, &qualified).segments[0].data, b"B2AX");
    let before_bad = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    let mut bad = qualified.clone();
    bad.mutation = request(run, ImsOperation::GetUnique, 7, &[], b"").mutation;
    bad.qualifiers[0].field = "MISSING".into();
    assert_eq!(call(service, run, &bad).status, "AK");
    assert_eq!(
        pcb::position(&service.lock().unwrap().state.sessions[run], 2),
        before_bad
    );
    let end = selected(run, ImsOperation::GetNext, 8, 2, &[], b"");
    assert_eq!(call(service, run, &end).segments[0].data, b"C2AZ");
    if !composite {
        assert_eq!(
            call(
                service,
                run,
                &selected(run, ImsOperation::GetNext, 9, 2, &["ROOT"], b"")
            )
            .segments[0]
                .data,
            b"A1ZX"
        );
    }
    assert_eq!(
        call(
            service,
            run,
            &selected(run, ImsOperation::GetNext, 10, 2, &["ROOT"], b"")
        )
        .status,
        "GE"
    );
    let exhausted = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    assert_eq!(exhausted.current(), None);
    assert_eq!(exhausted.parentage(), None);
    assert!(!exhausted.is_held());
    assert_eq!(
        call(
            service,
            run,
            &selected(run, ImsOperation::GetHoldUnique, 11, 3, &["ROOT"], b"")
        )
        .segments[0]
            .data,
        b"B2AX"
    );
    assert_eq!(
        pcb::position(&service.lock().unwrap().state.sessions[run], 2),
        exhausted
    );
    assert_eq!(service.lock().unwrap().state.sessions[run].position, prior);
}

#[test]
fn secondary_selected_pcb_navigation_and_failed_positions_memory() {
    let service = ImsService::open(
        Arc::new(MemoryStore::new(Default::default())),
        Default::default(),
    )
    .unwrap();
    exercise_secondary_positions(&service, false);
}

#[test]
fn secondary_composite_selected_pcb_navigation_memory() {
    let service = ImsService::open(
        Arc::new(MemoryStore::new(Default::default())),
        Default::default(),
    )
    .unwrap();
    exercise_secondary_positions(&service, true);
}

#[test]
fn secondary_composite_maintenance_sqlite() {
    let file = std::env::temp_dir().join(format!(
        "ims-index-maintenance-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    let service = ImsService::open(
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap()),
        Default::default(),
    )
    .unwrap();
    exercise_maintenance(&service);
    drop(service);
    let reopened = ImsService::open(
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap()),
        Default::default(),
    )
    .unwrap();
    assert_eq!(lookup(&reopened, b"AZ"), vec![b"A1ZX".to_vec()]);
    drop(reopened);
    std::fs::remove_file(file).unwrap();
}
