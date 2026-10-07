use super::*;

#[test]
fn ssa_last_direct_child_unsupported_operands_contexts_and_failed_parentage() {
    backends("operand-fences", |store, _| {
        let service = seeded(store.clone());
        assert_eq!(
            nav(
                &service,
                3,
                1,
                ImsOperation::GetHoldUnique,
                &[b"ROOT    (ROOTKEY EQA1)"]
            )
            .unwrap()
            .status,
            "  "
        );
        let before = snapshot(&service);
        let stored = rows(&*store);
        let cases: Vec<(ImsOperation, Vec<&[u8]>)> = vec![
            (ImsOperation::GetUnique, vec![LAST]),
            (ImsOperation::GetHoldUnique, vec![LAST]),
            (ImsOperation::GetNext, vec![LAST]),
            (ImsOperation::GetHoldNext, vec![LAST]),
            (ImsOperation::GetNextParent, vec![b"ROOT    *L "]),
            (
                ImsOperation::GetNextParent,
                vec![b"ROOT    (ROOTKEY EQA1)", LAST],
            ),
            (
                ImsOperation::GetNextParent,
                vec![b"ROOT    (ROOTKEY EQB2)", LAST],
            ),
            (
                ImsOperation::GetNextParent,
                vec![b"CHILD   *L(CHILDKEYEQC1)"],
            ),
            (ImsOperation::GetNextParent, vec![b"CHILD   *LL "]),
            (ImsOperation::GetNextParent, vec![b"CHILD   *LF "]),
            (ImsOperation::GetNextParent, vec![b"CHILD   *LU "]),
            (ImsOperation::GetNextParent, vec![b"CHILD   *LV "]),
            (ImsOperation::GetNextParent, vec![b"CHILD   *LW1 "]),
            (ImsOperation::GetNextParent, vec![b"CHILD   *LD "]),
            (ImsOperation::GetNextParent, vec![b"CHILD   *LP "]),
            (ImsOperation::GetNextParent, vec![b"CHILD   *-L-F- "]),
        ];
        for (i, (op, operands)) in cases.into_iter().enumerate() {
            assert_eq!(
                nav(&service, 10 + i as u64, 1, op, &operands),
                Err(HostProblem::Unsupported),
                "operand case {i}"
            );
            assert_eq!(snapshot(&service), before);
            assert_eq!(rows(&*store), stored);
        }
        for (i, context) in [
            ImsExecutionContext::DbDc,
            ImsExecutionContext::Dbctl,
            ImsExecutionContext::Dcctl,
            ImsExecutionContext::TmBatch,
        ]
        .into_iter()
        .enumerate()
        {
            let mut request = navigation(RUN, 40 + i as u64, ImsOperation::GetNextParent, &[LAST]);
            request.context = context;
            assert_eq!(
                public(service.clone(), RUN, request),
                Err(HostProblem::Unsupported)
            );
            assert_eq!(snapshot(&service), before);
            assert_eq!(rows(&*store), stored);
        }
        let null_last = nav(
            &service,
            45,
            1,
            ImsOperation::GetNextParent,
            &[b"CHILD   *-L- "],
        )
        .unwrap();
        assert_eq!(null_last.status, "  ");
        assert_eq!(null_last.segments[0].data, b"C3S");
        assert_eq!(null_last.segments[0].parent_key, Some(b"A1".to_vec()));
        assert_eq!(
            cursor(&service, 1),
            serde_json::json!({"current":5,"parentage":2,"held":null,"after_end":false})
        );
        let selected = snapshot(&service);
        let retained_rows = rows(&*store);
        assert_eq!(
            nav(&service, 45, 1, ImsOperation::GetNextParent, &[LAST]),
            Err(HostProblem::IdempotencyConflict),
            "null slots do not normalize the retained raw request identity"
        );
        assert_eq!(snapshot(&service), selected);
        assert_eq!(rows(&*store), retained_rows);
        assert_eq!(
            nav(
                &service,
                46,
                1,
                ImsOperation::GetUnique,
                &[b"ROOT    (ROOTKEY EQZ9)"]
            )
            .unwrap()
            .status,
            "GE"
        );
        let lost = snapshot(&service);
        assert_eq!(
            nav(&service, 47, 1, ImsOperation::GetNextParent, &[LAST]),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(snapshot(&service), lost);
        assert_eq!(
            nav(
                &service,
                48,
                1,
                ImsOperation::GetUnique,
                &[b"ROOT    (ROOTKEY EQA1)"]
            )
            .unwrap()
            .status,
            "  "
        );
        assert_eq!(
            nav(
                &service,
                49,
                1,
                ImsOperation::GetNext,
                &[b"ROOT    (ROOTKEY EQZ9)"]
            )
            .unwrap()
            .status,
            "GE"
        );
        let lost = snapshot(&service);
        assert_eq!(
            nav(&service, 50, 1, ImsOperation::GetNextParent, &[LAST]),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(snapshot(&service), lost);
    });
}

fn with_metadata(
    store: Arc<dyn ProviderStateStore>,
    metadata: ImsMetadataCatalog,
) -> Arc<ImsService> {
    let service = ImsService::open(store, ImsLimits::default()).unwrap();
    service.install_metadata(metadata).unwrap();
    load(&service, RUN);
    db(
        service.clone(),
        RUN,
        request(RUN, ImsOperation::Commit, 903, &[], b""),
    );
    service
}

#[test]
fn ssa_last_direct_child_physical_shape_and_selected_secondary_fences() {
    for kind in 0..7 {
        backends(&format!("shape-{kind}"), |store, _| {
            let mut metadata = last_catalog();
            match kind {
                0 => metadata.databases[0].organization = ImsDatabaseOrganization::Hdam,
                1 => metadata.databases[0].organization = ImsDatabaseOrganization::Dedb,
                2 => metadata.databases[0].segments[1].max_length = 4,
                3 => metadata.databases[0].segments[1].fields[0].unique = false,
                4 => metadata.databases[0].segments[1].fields[0].sequence = false,
                5 | 6 => {
                    let mut extra = metadata.databases[0].segments[1].clone();
                    extra.name = "EXTRA".into();
                    if kind == 5 {
                        extra.parent = Some("CHILD".into());
                    }
                    metadata.databases[0].segments.push(extra);
                }
                _ => unreachable!(),
            }
            let service = with_metadata(store.clone(), metadata);
            assert_eq!(
                nav(
                    &service,
                    3,
                    1,
                    ImsOperation::GetHoldUnique,
                    &[b"ROOT    (ROOTKEY EQA1)"]
                )
                .unwrap()
                .status,
                "  "
            );
            let before = snapshot(&service);
            let stored = rows(&*store);
            assert_eq!(
                nav(&service, 4, 1, ImsOperation::GetHoldNextParent, &[LAST]),
                Err(HostProblem::Unsupported)
            );
            assert_eq!(snapshot(&service), before);
            assert_eq!(rows(&*store), stored);
        });
    }
    backends("selected-secondary", |store, _| {
        let mut metadata = last_catalog();
        metadata.databases[0]
            .secondary_indexes
            .push(ImsSecondaryIndexMetadata {
                name: "BYKIND".into(),
                source_segment: "CHILD".into(),
                target_segment: "ROOT".into(),
                source_fields: vec!["KIND".into()],
            });
        let ImsPcbMetadata::Database(second) = &mut metadata.psbs[0].pcbs[1] else {
            unreachable!()
        };
        second.secondary_index = Some("BYKIND".into());
        let service = with_metadata(store.clone(), metadata);
        assert_eq!(
            nav(
                &service,
                3,
                2,
                ImsOperation::GetHoldUnique,
                &[b"ROOT    (BYKIND  EQQ)"]
            )
            .unwrap()
            .segments[0]
                .data,
            b"A1X"
        );
        let before = snapshot(&service);
        assert_eq!(
            nav(&service, 4, 2, ImsOperation::GetHoldNextParent, &[LAST]),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(snapshot(&service), before);
        // A primary PCB stays primary even when the database also has an index.
        assert_eq!(
            nav(
                &service,
                5,
                1,
                ImsOperation::GetUnique,
                &[b"ROOT    (ROOTKEY EQA1)"]
            )
            .unwrap()
            .status,
            "  "
        );
        assert_eq!(
            nav(&service, 6, 1, ImsOperation::GetNextParent, &[LAST])
                .unwrap()
                .segments[0]
                .data,
            b"C3S"
        );
    });
}

#[test]
fn ssa_last_direct_child_ac_am_key_only_and_saf_preserve_protected_state() {
    for condition in 0..3 {
        backends(&format!("condition-{condition}"), |store, _| {
            let mut metadata = last_catalog();
            let ImsPcbMetadata::Database(second) = &mut metadata.psbs[0].pcbs[1] else {
                unreachable!()
            };
            match condition {
                0 => {
                    second.sensitive_segments.pop();
                }
                1 => second.sensitive_segments[1].processing_options = Some("I".into()),
                _ => second.sensitive_segments[1].processing_options = Some("K".into()),
            }
            let service = with_metadata(store.clone(), metadata);
            nav(
                &service,
                3,
                2,
                ImsOperation::GetHoldUnique,
                &[b"ROOT    (ROOTKEY EQA1)"],
            )
            .unwrap();
            let before = position(&service, 2);
            let image = store
                .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
                .unwrap();
            let request = {
                let mut r = navigation(RUN, 4, ImsOperation::GetHoldNextParent, &[LAST]);
                r.request.pcb = 2;
                r
            };
            let result = public(service.clone(), RUN, request.clone()).unwrap();
            assert_eq!(result.status, ["AC", "AM", "  "][condition]);
            assert!(result.segments.is_empty());
            assert_eq!(
                store
                    .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
                    .unwrap()
                    .unwrap()
                    .payload,
                image.unwrap().payload
            );
            if condition < 2 {
                assert_eq!(position(&service, 2), before);
            } else {
                assert_eq!(
                    cursor(&service, 2),
                    serde_json::json!({"current":5,"parentage":2,"held":{"id":5,"version":1},"after_end":false})
                );
            }
            assert!(
                store
                    .get_provider_state(REPLAY_NAMESPACE, "ssa-last-run-4")
                    .unwrap()
                    .is_some()
            );
            let recorded = rows(&*store);
            assert_eq!(public(service.clone(), RUN, request).unwrap(), result);
            assert_eq!(rows(&*store), recorded);
        });
    }
    backends("saf", |store, _| {
        drop(seeded(store.clone()));
        let policy = Arc::new(Policy::default());
        let service =
            ImsService::open_authorized(store.clone(), ImsLimits::default(), policy.clone())
                .unwrap();
        nav(
            &service,
            3,
            1,
            ImsOperation::GetHoldUnique,
            &[b"ROOT    (ROOTKEY EQA1)"],
        )
        .unwrap();
        let request = navigation(RUN, 4, ImsOperation::GetHoldNextParent, &[LAST]);
        *policy.deny_update.lock().unwrap() = true;
        let before = snapshot(&service);
        let stored = rows(&*store);
        assert_eq!(
            public(service.clone(), RUN, request.clone()),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(snapshot(&service), before);
        assert_eq!(rows(&*store), stored);
        *policy.deny_update.lock().unwrap() = false;
        assert_eq!(
            public(service.clone(), RUN, request.clone())
                .unwrap()
                .segments[0]
                .data,
            b"C3S"
        );
        *policy.deny_update.lock().unwrap() = true;
        let before = snapshot(&service);
        let stored = rows(&*store);
        assert_eq!(
            public(service.clone(), RUN, request),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(snapshot(&service), before);
        assert_eq!(rows(&*store), stored);
    });
}

fn logical_catalog(remote: bool) -> ImsMetadataCatalog {
    let mut metadata = last_catalog();
    let mut parent = metadata.databases[0].clone();
    parent.name = "PARENTDB".into();
    parent.segments.truncate(1);
    parent.segments[0].name = "PROOT".into();
    let relationship = ImsLogicalRelationshipMetadata {
        parent_database: "PARENTDB".into(),
        parent_segment: "PROOT".into(),
        child_database: "GENDB".into(),
        child_segment: "CHILD".into(),
        paired: false,
    };
    if remote {
        parent.logical_relationships.push(relationship);
    } else {
        metadata.databases[0]
            .logical_relationships
            .push(relationship);
    }
    metadata.databases.push(parent);
    metadata
}

#[test]
fn ssa_last_direct_child_remote_local_logical_declaration_and_retained_link_fences() {
    for remote in [false, true] {
        backends(
            if remote {
                "logical-remote"
            } else {
                "logical-local"
            },
            |store, _| {
                let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
                let metadata = logical_catalog(remote);
                assert_eq!(
                    metadata.databases[0].logical_relationships.is_empty(),
                    remote
                );
                service.install_metadata(metadata).unwrap();
                let image = ImsGenericLoadImage {
                    database: "GENDB".into(),
                    records: vec![ImsGenericLoadRecord {
                        segment: "ROOT".into(),
                        parent: None,
                        data: b"A1X".to_vec(),
                    }],
                };
                db(
                    service.clone(),
                    RUN,
                    request(
                        RUN,
                        ImsOperation::Load,
                        1,
                        &[],
                        &serde_json::to_vec(&image).unwrap(),
                    ),
                );
                db(
                    service.clone(),
                    RUN,
                    request(RUN, ImsOperation::Schedule, 2, &[], b""),
                );
                db(
                    service.clone(),
                    RUN,
                    request(RUN, ImsOperation::Commit, 903, &[], b""),
                );
                assert_eq!(
                    nav(
                        &service,
                        3,
                        1,
                        ImsOperation::GetHoldUnique,
                        &[b"ROOT    (ROOTKEY EQA1)"]
                    )
                    .unwrap()
                    .segments[0]
                        .data,
                    b"A1X"
                );
                let before = snapshot(&service);
                let stored = rows(&*store);
                assert_eq!(
                    nav(&service, 4, 1, ImsOperation::GetHoldNextParent, &[LAST]),
                    Err(HostProblem::Unsupported)
                );
                assert_eq!(snapshot(&service), before);
                assert_eq!(rows(&*store), stored);
                assert!(position(&service, 1).is_held());
                let parent = ImsGenericLoadImage {
                    database: "PARENTDB".into(),
                    records: vec![ImsGenericLoadRecord {
                        segment: "PROOT".into(),
                        parent: None,
                        data: b"P1Z".to_vec(),
                    }],
                };
                db(
                    service.clone(),
                    "last-parent-loader",
                    request(
                        "last-parent-loader",
                        ImsOperation::Load,
                        1,
                        &[],
                        &serde_json::to_vec(&parent).unwrap(),
                    ),
                );
                db(
                    service.clone(),
                    "last-parent-loader",
                    request("last-parent-loader", ImsOperation::Commit, 903, &[], b""),
                );
                let mut insert = request(RUN, ImsOperation::Insert, 5, &["CHILD"], b"C1Q");
                insert.qualifiers.push(ImsQualifier {
                    segment: "PROOT".into(),
                    field: "ROOTKEY".into(),
                    value: b"P1".to_vec(),
                });
                assert_eq!(db(service.clone(), RUN, insert).status, "  ");
                nav(
                    &service,
                    6,
                    1,
                    ImsOperation::GetHoldUnique,
                    &[b"ROOT    (ROOTKEY EQA1)"],
                )
                .unwrap();
                let before = snapshot(&service);
                assert_eq!(
                    nav(&service, 7, 1, ImsOperation::GetHoldNextParent, &[LAST]),
                    Err(HostProblem::Unsupported)
                );
                assert_eq!(snapshot(&service), before);
                let durable = service.lock().unwrap();
                let engine = restored(&durable.state, "GENDB", ImsLimits::default()).unwrap();
                assert_eq!(engine.logical_links().len(), 1);
                let fields = engine.ssa_fields(None);
                let ssa = mainframe_env_host_api::parse_ims_ssa(
                    LAST,
                    mainframe_env_host_api::ImsSsaLimits::default(),
                    &fields,
                )
                .unwrap();
                let read = ReadRequest {
                    kind: ReadKind::NextInParent,
                    target: Some("CHILD".into()),
                    path: vec![SegmentSelector {
                        segment: "CHILD".into(),
                        predicates: vec![],
                    }],
                    hold: true,
                };
                let position =
                    super::super::super::super::pcb::position(&durable.state.sessions[RUN], 1);
                assert_eq!(
                    engine.validate_last_direct_child(&position, &read, &[ssa], None),
                    Err(EngineProblem::Unsupported),
                    "live links are independently excluded in the engine guard"
                );
            },
        );
    }
}

#[test]
fn ssa_last_direct_child_real_current_and_root_deletion_cancel_authority() {
    for root in [false, true] {
        backends(
            if root {
                "delete-root"
            } else {
                "delete-current"
            },
            |store, _| {
                let service = seeded(store.clone());
                nav(
                    &service,
                    3,
                    1,
                    ImsOperation::GetHoldUnique,
                    &[b"ROOT    (ROOTKEY EQA1)"],
                )
                .unwrap();
                if !root {
                    nav(
                        &service,
                        4,
                        1,
                        ImsOperation::GetHoldNextParent,
                        &[b"CHILD    "],
                    )
                    .unwrap();
                }
                let operands: Vec<&[u8]> = if root {
                    vec![b"ROOT    (ROOTKEY EQA1)"]
                } else {
                    vec![b"ROOT    (ROOTKEY EQA1)", b"CHILD   (CHILDKEYEQC1)"]
                };
                nav(&service, 40, 2, ImsOperation::GetHoldUnique, &operands).unwrap();
                let mut delete = request(RUN, ImsOperation::Delete, 5, &[], b"");
                delete.pcb = 2;
                assert_eq!(db(service.clone(), RUN, delete).status, "  ");
                let before = snapshot(&service);
                let stored = rows(&*store);
                assert_eq!(
                    nav(&service, 6, 1, ImsOperation::GetNextParent, &[LAST]),
                    Err(HostProblem::Unsupported)
                );
                assert_eq!(snapshot(&service), before);
                assert_eq!(rows(&*store), stored);
            },
        );
    }
}
