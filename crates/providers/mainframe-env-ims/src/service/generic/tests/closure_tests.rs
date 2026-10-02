use super::*;
use mainframe_env_store::StoreLimits;

#[test]
fn public_route_selects_every_pinned_organization_from_metadata() {
    use ImsDatabaseOrganization::*;
    for organization in [
        Dedb, Gsam, Hdam, Hidam, Hisam, Hsam, Index, Msdb, Phdam, Phidam, Psindex, Shisam, Shsam,
    ] {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service = ImsService::open(store, ImsLimits::default()).unwrap();
        let mut metadata = catalog();
        let database = &mut metadata.databases[0];
        database.organization = organization;
        database.segments.truncate(1);
        if organization == Gsam {
            database.segments[0].fields[0].sequence = false;
        }
        metadata.psbs[0].pcbs[0] = ImsPcbMetadata::Database(ImsDatabasePcbMetadata {
            name: "GENPCB".into(),
            database: "GENDB".into(),
            database_version: Some(1),
            secondary_index: None,
            processing_options: "AP".into(),
            sensitive_segments: vec![ImsSensitiveSegmentMetadata {
                name: "ROOT".into(),
                parent: None,
                processing_options: None,
            }],
        });
        service.install_metadata(metadata).unwrap();
        let run = "org-run";
        let scheduled = execute(
            &service,
            run,
            &request(run, ImsOperation::Schedule, 1, &[], b""),
        );
        assert_eq!(scheduled.status, "  ", "{organization:?}");
        let index_database = matches!(organization, Index | Psindex);
        assert_eq!(
            execute(
                &service,
                run,
                &request(run, ImsOperation::Insert, 2, &["ROOT"], b"R1A")
            )
            .status,
            if index_database { "AC" } else { "  " },
            "{organization:?}"
        );
        if index_database {
            let image = ImsGenericLoadImage {
                database: "GENDB".into(),
                records: vec![ImsGenericLoadRecord {
                    segment: "ROOT".into(),
                    parent: None,
                    data: b"R1A".to_vec(),
                }],
            };
            assert_eq!(
                execute(
                    &service,
                    run,
                    &request(
                        run,
                        ImsOperation::Load,
                        3,
                        &[],
                        &serde_json::to_vec(&image).unwrap()
                    )
                )
                .affected_segments,
                1
            );
        }
        let read_op = if organization == Gsam {
            ImsOperation::GetNext
        } else {
            ImsOperation::GetUnique
        };
        let read_run = if organization == Gsam {
            // A different normal PCB cannot read this writer's pending image.
            execute(
                &service,
                run,
                &request(run, ImsOperation::Commit, 3, &[], b""),
            );
            let fresh = "org-read";
            assert_eq!(
                execute(
                    &service,
                    fresh,
                    &request(fresh, ImsOperation::Schedule, 1, &[], b"")
                )
                .status,
                "  "
            );
            fresh
        } else {
            run
        };
        assert_eq!(
            execute(
                &service,
                read_run,
                &request(read_run, read_op, 4, &["ROOT"], b"")
            )
            .segments[0]
                .data,
            b"R1A",
            "{organization:?}"
        );
        if organization == Gsam {
            assert_eq!(
                execute(
                    &service,
                    run,
                    &request(run, ImsOperation::GetHoldUnique, 5, &["ROOT"], b"")
                )
                .status,
                "AC"
            );
            continue;
        }
        assert_eq!(
            execute(
                &service,
                run,
                &request(run, ImsOperation::GetHoldUnique, 5, &["ROOT"], b"")
            )
            .status,
            "  ",
            "{organization:?}"
        );
        assert_eq!(
            execute(
                &service,
                run,
                &request(run, ImsOperation::Replace, 6, &[], b"R1Z")
            )
            .status,
            "  ",
            "{organization:?}"
        );
        assert_eq!(
            execute(
                &service,
                run,
                &request(run, ImsOperation::Delete, 7, &[], b"")
            )
            .affected_segments,
            1,
            "{organization:?}"
        );
    }
}

fn relationship_catalog(paired: bool) -> ImsMetadataCatalog {
    let mut metadata = catalog();
    metadata.databases[0]
        .logical_relationships
        .push(ImsLogicalRelationshipMetadata {
            parent_database: "PARENTDB".into(),
            parent_segment: "PROOT".into(),
            child_database: "GENDB".into(),
            child_segment: "CHILD".into(),
            paired,
        });
    metadata.databases.push(ImsDatabaseMetadata {
        name: "PARENTDB".into(),
        version: 1,
        organization: ImsDatabaseOrganization::Phidam,
        segments: vec![ImsSegmentMetadata {
            name: "PROOT".into(),
            parent: None,
            min_length: 3,
            max_length: 3,
            fields: vec![
                ImsFieldMetadata {
                    name: Some("PKEY".into()),
                    offset: 0,
                    length: 2,
                    sequence: true,
                    unique: true,
                },
                ImsFieldMetadata {
                    name: Some("VALUE".into()),
                    offset: 2,
                    length: 1,
                    sequence: false,
                    unique: false,
                },
            ],
        }],
        secondary_indexes: vec![],
        logical_relationships: vec![],
    });
    metadata.psbs[0]
        .pcbs
        .push(ImsPcbMetadata::Database(ImsDatabasePcbMetadata {
            name: "PPCB".into(),
            database: "PARENTDB".into(),
            database_version: Some(1),
            secondary_index: None,
            processing_options: "AP".into(),
            sensitive_segments: vec![ImsSensitiveSegmentMetadata {
                name: "PROOT".into(),
                parent: None,
                processing_options: None,
            }],
        }));
    metadata
}

pub(super) fn setup(service: &ImsService, paired: bool) {
    service
        .install_metadata(relationship_catalog(paired))
        .unwrap();
    let parent = "parent-run";
    let child = "child-run";
    let mut schedule = request(parent, ImsOperation::Schedule, 1, &[], b"");
    schedule.pcb = 2;
    assert_eq!(execute(service, parent, &schedule).status, "  ");
    assert_eq!(
        execute(
            service,
            child,
            &request(child, ImsOperation::Schedule, 1, &[], b"")
        )
        .status,
        "  "
    );
    assert_eq!(
        execute(
            service,
            parent,
            &parent_request(ImsOperation::Insert, 2, &["PROOT"], b"P1A")
        )
        .status,
        "  "
    );
    execute(
        service,
        parent,
        &request(parent, ImsOperation::Commit, 100, &[], b""),
    );
    assert_eq!(
        execute(
            service,
            child,
            &request(child, ImsOperation::Insert, 2, &["ROOT"], b"R1A")
        )
        .status,
        "  "
    );
}

pub(super) fn insert_child(service: &ImsService, sequence: u64) -> ImsRequest {
    let run = "child-run";
    let mut insert = request(run, ImsOperation::Insert, sequence, &["CHILD"], b"C1B");
    insert.qualifiers.push(ImsQualifier {
        segment: "PROOT".into(),
        field: "PKEY".into(),
        value: b"P1".to_vec(),
    });
    assert_eq!(execute(service, run, &insert).status, "  ");
    insert
}

pub(super) fn hold_parent(service: &ImsService, sequence: u64) {
    let run = "parent-run";
    let mut hold = parent_request(ImsOperation::GetHoldUnique, sequence, &["PROOT"], b"");
    hold.qualifiers.push(ImsQualifier {
        segment: "PROOT".into(),
        field: "PKEY".into(),
        value: b"P1".to_vec(),
    });
    assert_eq!(execute(service, run, &hold).status, "  ");
}

fn parent_request(op: ImsOperation, sequence: u64, segments: &[&str], data: &[u8]) -> ImsRequest {
    let mut request = request("parent-run", op, sequence, segments, data);
    request.pcb = 2;
    request
}

#[test]
fn logical_child_requires_resolved_parent_and_tracks_parent_replace() {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    setup(&service, false);
    let before = restored(
        &service.lock().unwrap().state,
        "GENDB",
        ImsLimits::default(),
    )
    .unwrap()
    .state_digest();
    assert_eq!(
        execute(
            &service,
            "child-run",
            &request("child-run", ImsOperation::Insert, 3, &["CHILD"], b"C1B")
        )
        .status,
        "GP"
    );
    assert_eq!(
        restored(
            &service.lock().unwrap().state,
            "GENDB",
            ImsLimits::default()
        )
        .unwrap()
        .state_digest(),
        before
    );
    let insert = insert_child(&service, 4);
    assert_eq!(execute(&service, "child-run", &insert).status, "  ");
    assert_eq!(
        execute(
            &service,
            "child-run",
            &request("child-run", ImsOperation::GetUnique, 5, &["CHILD"], b"")
        )
        .segments[0]
            .data,
        b"C1BP1A"
    );
    execute(
        &service,
        "child-run",
        &request("child-run", ImsOperation::Commit, 100, &[], b""),
    );
    hold_parent(&service, 6);
    assert_eq!(
        execute(
            &service,
            "parent-run",
            &parent_request(ImsOperation::Replace, 7, &[], b"P1Z")
        )
        .status,
        "  "
    );
    let read = request("child-run", ImsOperation::GetUnique, 8, &["CHILD"], b"");
    assert_eq!(
        service.execute(&invocation("child-run"), &read),
        Err(HostProblem::IdempotencyConflict)
    );
    execute(
        &service,
        "parent-run",
        &request("parent-run", ImsOperation::Commit, 9, &[], b""),
    );
    assert_eq!(
        execute(&service, "child-run", &read).segments[0].data,
        b"C1BP1Z"
    );
    assert!(ImsService::open(store, ImsLimits::default()).is_ok());
}

#[test]
fn unpaired_parent_delete_fails_without_partial_mutation() {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let service = ImsService::open(store, ImsLimits::default()).unwrap();
    setup(&service, false);
    insert_child(&service, 3);
    execute(
        &service,
        "child-run",
        &request("child-run", ImsOperation::Commit, 100, &[], b""),
    );
    hold_parent(&service, 4);
    let before = service.lock().unwrap().state.generic_databases.clone();
    assert_eq!(
        execute(
            &service,
            "parent-run",
            &parent_request(ImsOperation::Delete, 5, &[], b"")
        )
        .status,
        "GP"
    );
    assert_eq!(service.lock().unwrap().state.generic_databases, before);
}

#[test]
fn paired_delete_rolls_back_both_databases_and_reopens_from_sqlite() {
    let file = std::env::temp_dir().join(format!(
        "ims-logical-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    {
        let store: Arc<dyn ProviderStateStore> =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let service = ImsService::open(store, ImsLimits::default()).unwrap();
        setup(&service, true);
        insert_child(&service, 3);
        execute(
            &service,
            "child-run",
            &request("child-run", ImsOperation::Commit, 4, &[], b""),
        );
        execute(
            &service,
            "parent-run",
            &request("parent-run", ImsOperation::Commit, 4, &[], b""),
        );
        hold_parent(&service, 5);
        assert_eq!(
            execute(
                &service,
                "parent-run",
                &parent_request(ImsOperation::Delete, 6, &[], b"")
            )
            .affected_segments,
            2
        );
        assert_eq!(
            service.execute(
                &invocation("child-run"),
                &request("child-run", ImsOperation::GetUnique, 7, &["CHILD"], b"")
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        execute(
            &service,
            "parent-run",
            &request("parent-run", ImsOperation::Rollback, 8, &[], b""),
        );
        assert_eq!(
            execute(
                &service,
                "child-run",
                &request("child-run", ImsOperation::GetUnique, 9, &["CHILD"], b"")
            )
            .segments[0]
                .data,
            b"C1BP1A"
        );
        hold_parent(&service, 10);
        execute(
            &service,
            "parent-run",
            &parent_request(ImsOperation::Delete, 11, &[], b""),
        );
        execute(
            &service,
            "parent-run",
            &request("parent-run", ImsOperation::Commit, 12, &[], b""),
        );
    }
    {
        let store: Arc<dyn ProviderStateStore> =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let service = ImsService::open(store, ImsLimits::default()).unwrap();
        assert_eq!(
            execute(
                &service,
                "child-run",
                &request("child-run", ImsOperation::GetUnique, 13, &["CHILD"], b"")
            )
            .status,
            "GE"
        );
    }
    std::fs::remove_file(file).unwrap();
}

#[test]
fn corrupted_logical_parent_reference_is_rejected_on_reopen() {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    setup(&service, true);
    insert_child(&service, 3);
    drop(service);
    let mut row = store
        .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
        .unwrap()
        .unwrap();
    let mut payload: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
    payload["value"]["logical_links"][0]["parent"] = serde_json::json!(999);
    row.payload = serde_json::to_vec(&payload).unwrap();
    let version = row.version;
    row.version += 1;
    store.put_provider_state(row, Some(version)).unwrap();
    assert!(matches!(
        ImsService::open(store, ImsLimits::default()),
        Err(HostProblem::InfrastructureFailure)
    ));
}

#[test]
fn paired_multi_database_write_is_atomic_when_store_capacity_is_exhausted() {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(StoreLimits {
        max_provider_state: 64,
        ..Default::default()
    }));
    let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    setup(&service, true);
    insert_child(&service, 3);
    execute(
        &service,
        "child-run",
        &request("child-run", ImsOperation::Commit, 4, &[], b""),
    );
    execute(
        &service,
        "parent-run",
        &request("parent-run", ImsOperation::Commit, 4, &[], b""),
    );
    hold_parent(&service, 5);
    let before = service.lock().unwrap().state.generic_databases.clone();
    let parent_row = store
        .get_provider_state(GENERIC_DATABASE_NAMESPACE, "PARENTDB")
        .unwrap()
        .unwrap();
    let child_row = store
        .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
        .unwrap()
        .unwrap();
    let count = store.list_provider_state_prefix("ims", 128).unwrap().len();
    for number in count..64 {
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "test-fill".into(),
                    key: format!("{number}"),
                    version: 1,
                    payload: vec![1],
                },
                None,
            )
            .unwrap();
    }
    assert!(matches!(
        service.execute(
            &invocation("parent-run"),
            &parent_request(ImsOperation::Delete, 6, &[], b"")
        ),
        Err(HostProblem::ResourceExhausted)
    ));
    assert_eq!(service.lock().unwrap().state.generic_databases, before);
    assert_eq!(
        store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "PARENTDB")
            .unwrap(),
        Some(parent_row)
    );
    assert_eq!(
        store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
            .unwrap(),
        Some(child_row)
    );
}
