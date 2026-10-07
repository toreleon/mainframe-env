use super::*;

fn logical_catalog() -> ImsMetadataCatalog {
    let mut metadata = catalog();
    let mut destination = metadata.databases[0].clone();
    destination.name = "PARENTDB".into();
    destination.segments[0].name = "DROOT".into();
    destination.segments[1].name = "LPARENT".into();
    destination.segments[1].parent = Some("DROOT".into());
    destination.segments[0].fields[0].name = Some("DROOTKEY".into());
    destination.segments[1].fields[0].name = Some("LPKEY".into());
    metadata.databases[0].logical_relationships = vec![ImsLogicalRelationshipMetadata {
        child_database: "GENDB".into(),
        child_segment: "CHILD".into(),
        parent_database: "PARENTDB".into(),
        parent_segment: "LPARENT".into(),
        paired: true,
    }];
    metadata.databases.push(destination);
    let ImsPcbMetadata::Database(source) = metadata.psbs[0].pcbs[0].clone() else {
        unreachable!()
    };
    let mut parent = source.clone();
    parent.name = "PARENTPC".into();
    parent.database = "PARENTDB".into();
    parent.sensitive_segments[0].name = "DROOT".into();
    parent.sensitive_segments[1].name = "LPARENT".into();
    parent.sensitive_segments[1].parent = Some("DROOT".into());
    metadata.psbs[0].pcbs.push(ImsPcbMetadata::Database(parent));
    let mut independent = source.clone();
    independent.name = "OTHERPCB".into();
    metadata.psbs[0]
        .pcbs
        .push(ImsPcbMetadata::Database(independent));
    let mut key_only = source;
    key_only.name = "KEYPCB".into();
    key_only.sensitive_segments[1].processing_options = Some("K".into());
    metadata.psbs[0]
        .pcbs
        .push(ImsPcbMetadata::Database(key_only));
    metadata
}

fn q(segment: &str, field: &str, value: &[u8]) -> ImsQualifier {
    ImsQualifier {
        segment: segment.into(),
        field: field.into(),
        value: value.into(),
    }
}

fn seed_logical(store: Arc<dyn ProviderStateStore>, run: &str) -> Arc<ImsService> {
    seed_catalog(store, run, logical_catalog())
}

fn seed_catalog(
    store: Arc<dyn ProviderStateStore>,
    run: &str,
    metadata: ImsMetadataCatalog,
) -> Arc<ImsService> {
    let service = ImsService::open(store, Default::default()).unwrap();
    service.install_metadata(metadata).unwrap();
    let seed = "logical-seed";
    assert_eq!(
        execute(
            &service,
            seed,
            &request(seed, ImsOperation::Schedule, 1, &[], b"")
        )
        .status,
        "  "
    );
    for (seq, pcb, name, data, qualifiers) in [
        (2, 2, "DROOT", b"P9A".as_slice(), vec![]),
        (3, 2, "LPARENT", b"L2X", vec![q("DROOT", "DROOTKEY", b"P9")]),
        (4, 2, "DROOT", b"Q8B", vec![]),
        (5, 2, "LPARENT", b"L2Y", vec![q("DROOT", "DROOTKEY", b"Q8")]),
        (6, 1, "ROOT", b"A1X", vec![]),
        (7, 1, "ROOT", b"B2Y", vec![]),
        (
            8,
            1,
            "CHILD",
            b"C1Z",
            vec![
                qualifier(b"A1"),
                q("LPARENT", "KIND", b"X"),
                q("DROOT", "DROOTKEY", b"P9"),
            ],
        ),
        (
            9,
            1,
            "CHILD",
            b"D2W",
            vec![
                qualifier(b"A1"),
                q("LPARENT", "KIND", b"Y"),
                q("DROOT", "DROOTKEY", b"P9"),
            ],
        ),
        (
            10,
            1,
            "CHILD",
            b"C1V",
            vec![
                qualifier(b"B2"),
                q("LPARENT", "KIND", b"Y"),
                q("DROOT", "DROOTKEY", b"P9"),
            ],
        ),
    ] {
        let mut req = request(seed, ImsOperation::Insert, seq, &[name], data);
        req.pcb = pcb;
        req.qualifiers = qualifiers;
        assert_eq!(execute(&service, seed, &req).status, "  ", "seed {seq}");
    }
    assert_eq!(
        execute(
            &service,
            seed,
            &request(seed, ImsOperation::Commit, 11, &[], b"")
        )
        .status,
        "  "
    );
    assert_eq!(
        execute(
            &service,
            run,
            &request(run, ImsOperation::Schedule, 1, &[], b"")
        )
        .status,
        "  "
    );
    service
}

fn call(
    service: Arc<ImsService>,
    run: &str,
    req: ImsPcbFeedbackRequestV1,
) -> Result<ImsPcbFeedbackResultV1, HostProblem> {
    let inv = invocation_class(run, ServiceClass::Batch);
    let request = HostRequest::ImsPcbFeedbackV1(req);
    request.validate(HostLimits::default())?;
    let provider = ims_providers(service, InvocationLimits::default()).remove(1);
    match provider
        .invoke(
            &inv,
            EffectRequest {
                run_unit: inv.run_unit_id.clone(),
                sequence: request.mutation().unwrap().sequence,
                deadline_tick: inv.deadline_tick,
                idempotency_key: request.mutation().map(|m| m.idempotency_key.clone()),
                request,
            },
        )
        .outcome?
    {
        HostResult::ImsPcbFeedbackV1(result) => Ok(result),
        _ => panic!("wrong logical feedback result"),
    }
}

fn child_c(run: &str, seq: u64, pcb: u16, key: &[u8]) -> ImsPcbFeedbackRequestV1 {
    let mut req = feedback(run, seq, ImsOperation::GetUnique, pcb, &[], b"");
    let mut ssa = b"CHILD   *C(".to_vec();
    ssa.extend_from_slice(key);
    ssa.push(b')');
    req.ssas = Some(vec![ssa]);
    req
}

fn assert_source_position(service: &ImsService, run: &str, pcb: u16, data: &[&[u8]]) {
    let durable = service.lock().unwrap();
    let position = generic::pcb::position(&durable.state.sessions[run], pcb);
    let engine = generic::restored(&durable.state, "GENDB", Default::default()).unwrap();
    let path = engine.path_to(position.current().unwrap()).unwrap();
    assert_eq!(
        path.iter()
            .map(|view| view.data.as_slice())
            .collect::<Vec<_>>(),
        data
    );
}

#[test]
fn logical_navigation_public_physical_key_not_destination_key() {
    for store in failure_tests::backends() {
        let run = "logical-feedback-red";
        let service = seed_logical(store, run);
        let got = call(service.clone(), run, child_c(run, 2, 1, b"A1C1")).unwrap();
        // Authored from source field bytes; destination P9L2/Q8L2 is not this key.
        assert_eq!(got.result.segments[0].data, b"C1ZL2X");
        assert_key(&got, "CHILD", 2, b"A1C1", 6);
        assert_source_position(&service, run, 1, &[b"A1X", b"C1Z"]);
        assert_eq!(got.feedback.database, "GENDB");
        assert_eq!(got.feedback.processing_options, "AP");
        assert_eq!(got.feedback.sensitive_segment_count, 2);
        let source = generic::pcb::position(&service.lock().unwrap().state.sessions[run], 1);
        let other = call(service.clone(), run, child_c(run, 3, 3, b"B2C1")).unwrap();
        assert_eq!(other.result.segments[0].data, b"C1VL2Y");
        assert_key(&other, "CHILD", 2, b"B2C1", 6);
        assert_source_position(&service, run, 3, &[b"B2Y", b"C1V"]);
        assert_eq!(
            generic::pcb::position(&service.lock().unwrap().state.sessions[run], 1),
            source
        );
    }
}

#[test]
fn logical_navigation_six_gets_failures_ordinary_paths_and_pcb_isolation() {
    for store in failure_tests::backends() {
        let run = "logical-six";
        let service = seed_logical(store, run);
        let mut seq = 2;
        for op in [
            ImsOperation::GetUnique,
            ImsOperation::GetHoldUnique,
            ImsOperation::GetNext,
            ImsOperation::GetHoldNext,
            ImsOperation::GetNextParent,
            ImsOperation::GetHoldNextParent,
        ] {
            let root = call(
                service.clone(),
                run,
                feedback(run, seq, ImsOperation::GetUnique, 1, &["ROOT"], b""),
            )
            .unwrap();
            assert_key(&root, "ROOT", 1, b"A1", 3);
            seq += 1;
            let child = call(
                service.clone(),
                run,
                feedback(run, seq, op, 1, &["CHILD"], b""),
            )
            .unwrap();
            assert_key(&child, "CHILD", 2, b"A1C1", 6);
            assert_source_position(&service, run, 1, &[b"A1X", b"C1Z"]);
            assert_eq!(child.result.segments[0].data, b"C1ZL2X");
            let position = generic::pcb::position(&service.lock().unwrap().state.sessions[run], 1);
            assert_eq!(
                position.is_held(),
                matches!(
                    op,
                    ImsOperation::GetHoldUnique
                        | ImsOperation::GetHoldNext
                        | ImsOperation::GetHoldNextParent
                )
            );
            seq += 1;
            let other = call(service.clone(), run, child_c(run, seq, 3, b"B2C1")).unwrap();
            assert_key(&other, "CHILD", 2, b"B2C1", 6);
            assert_eq!(
                generic::pcb::position(&service.lock().unwrap().state.sessions[run], 1),
                position
            );
            seq += 1;
            call(
                service.clone(),
                run,
                feedback(run, seq, ImsOperation::GetUnique, 1, &["ROOT"], b""),
            )
            .unwrap();
            seq += 1;
            let mut missing = feedback(run, seq, op, 1, &["CHILD"], b"");
            missing
                .request
                .qualifiers
                .push(q("CHILD", "CHILDKEY", b"NO"));
            let failed = call(service.clone(), run, missing).unwrap();
            assert_eq!(failed.result.status, "GE");
            assert_eq!(
                failed.feedback.key,
                ImsPcbKeyFeedbackV1::Unsupported(Unproved::FailedCallWitness)
            );
            assert_eq!(failed.feedback.transferred_data_length, 0);
            seq += 1;
        }
        call(
            service.clone(),
            run,
            feedback(run, seq, ImsOperation::GetUnique, 1, &["ROOT"], b""),
        )
        .unwrap();
        seq += 1;
        let sibling = call(
            service.clone(),
            run,
            feedback(run, seq, ImsOperation::GetNextParent, 1, &["CHILD"], b""),
        )
        .unwrap();
        assert_key(&sibling, "CHILD", 2, b"A1C1", 6);
        seq += 1;
        let sibling = call(
            service.clone(),
            run,
            feedback(run, seq, ImsOperation::GetNextParent, 1, &["CHILD"], b""),
        )
        .unwrap();
        assert_key(&sibling, "CHILD", 2, b"A1D2", 6);
        assert_eq!(sibling.result.segments[0].data, b"D2WL2Y");
        seq += 1;
        let mut parent = feedback(
            run,
            seq,
            ImsOperation::GetUnique,
            2,
            &["DROOT", "LPARENT"],
            b"",
        );
        parent.request.qualifiers =
            vec![q("DROOT", "DROOTKEY", b"Q8"), q("LPARENT", "LPKEY", b"L2")];
        let parent = call(service, run, parent).unwrap();
        assert_key(&parent, "LPARENT", 2, b"Q8L2", 3);
        assert_eq!(parent.result.segments[0].data, b"L2Y");
    }
}

#[test]
fn logical_navigation_qualified_c_path_key_only_and_binary_keys() {
    for store in failure_tests::backends() {
        let run = "logical-path";
        let service = seed_logical(store, run);
        let mut req = feedback(run, 2, ImsOperation::GetUnique, 1, &["ROOT", "CHILD"], b"");
        req.request.qualifiers = vec![qualifier(b"B2"), q("CHILD", "CHILDKEY", b"C1")];
        let qualified = call(service.clone(), run, req).unwrap();
        assert_key(&qualified, "CHILD", 2, b"B2C1", 6);
        let c = call(service.clone(), run, child_c(run, 3, 1, b"B2C1")).unwrap();
        assert_eq!(c.result.segments, qualified.result.segments);
        assert_eq!(c.feedback.key, qualified.feedback.key);
        let mut path = feedback(run, 4, ImsOperation::GetUnique, 1, &[], b"");
        path.ssas = Some(vec![b"ROOT    *D ".to_vec(), b"CHILD    ".to_vec()]);
        let data = call(service.clone(), run, path.clone()).unwrap();
        assert_key(&data, "CHILD", 2, b"A1C1", 9);
        assert_eq!(
            data.result
                .segments
                .iter()
                .map(|s| s.data.as_slice())
                .collect::<Vec<_>>(),
            [b"A1X".as_slice(), b"C1ZL2X"]
        );
        path.request.pcb = 4;
        path.request.mutation = request(run, ImsOperation::GetUnique, 5, &[], b"").mutation;
        let hidden = call(service.clone(), run, path).unwrap();
        assert_key(&hidden, "CHILD", 2, b"A1C1", 3);
        assert_eq!(hidden.result.segments[0].data, b"A1X");
        assert_eq!(hidden.result.segments.len(), 1);
        let only = call(service.clone(), run, child_c(run, 6, 4, b"A1C1")).unwrap();
        assert_key(&only, "CHILD", 2, b"A1C1", 0);
        assert!(only.result.segments.is_empty());
        let skipped = call(
            service.clone(),
            run,
            feedback(run, 7, ImsOperation::GetNext, 4, &[], b""),
        )
        .unwrap();
        assert_key(&skipped, "ROOT", 1, b"B2", 3);
        for (seq, name, data, qualifiers) in [
            (8, "ROOT", [0, 0xff, b'R'], vec![]),
            (
                9,
                "CHILD",
                [0x80, 0, b'C'],
                vec![qualifier(&[0, 0xff]), q("LPARENT", "KIND", b"X")],
            ),
        ] {
            let mut insert = request(run, ImsOperation::Insert, seq, &[name], &data);
            insert.qualifiers = qualifiers;
            assert_eq!(execute(&service, run, &insert).status, "  ");
        }
        let binary = call(service, run, child_c(run, 10, 1, &[0, 0xff, 0x80, 0])).unwrap();
        assert_key(&binary, "CHILD", 2, &[0, 0xff, 0x80, 0], 6);
        assert_eq!(
            binary.result.segments[0].data,
            [0x80, 0, b'C', b'L', b'2', b'X']
        );
    }
}

#[test]
fn logical_navigation_unproved_classes_keep_unsupported() {
    for store_factory in [0, 1] {
        for class in [
            "unkeyed",
            "nonunique",
            "multiple",
            "variable-source",
            "variable-destination",
            "hdam",
            "parent-declaration",
        ] {
            let store: Arc<dyn ProviderStateStore> = if store_factory == 0 {
                Arc::new(MemoryStore::new(Default::default()))
            } else {
                failure_tests::backends().remove(1)
            };
            let mut m = logical_catalog();
            match class {
                "unkeyed" => {
                    m.databases[0].segments[1].fields[0].sequence = false;
                    m.databases[0].segments[1].fields[0].unique = false;
                }
                "nonunique" => m.databases[0].segments[1].fields[0].unique = false,
                "multiple" => {
                    let mut r = m.databases[0].logical_relationships[0].clone();
                    r.parent_segment = "DROOT".into();
                    m.databases[0].logical_relationships.push(r);
                }
                "variable-source" => m.databases[0].segments[1].min_length = 2,
                "variable-destination" => m.databases[1].segments[1].min_length = 2,
                "hdam" => m.databases[0].organization = ImsDatabaseOrganization::Hdam,
                "parent-declaration" => {
                    m.databases[1].logical_relationships =
                        std::mem::take(&mut m.databases[0].logical_relationships)
                }
                _ => unreachable!(),
            }
            let run = "logical-classes";
            let service = seed_catalog(store, run, m);
            let got = call(
                service.clone(),
                run,
                feedback(run, 2, ImsOperation::GetUnique, 1, &["CHILD"], b""),
            )
            .unwrap();
            assert_eq!(got.result.status, "  ", "{class}");
            if class == "parent-declaration" {
                assert_key(&got, "CHILD", 2, b"A1C1", 6);
            } else {
                assert_eq!(
                    got.feedback.key,
                    ImsPcbKeyFeedbackV1::Unsupported(Unproved::LogicalRelationship),
                    "{class}"
                );
            }
            let mut other_context = feedback(run, 3, ImsOperation::GetUnique, 1, &["CHILD"], b"");
            other_context.context = ImsExecutionContext::DbDc;
            assert_eq!(
                call(service.clone(), run, other_context)
                    .unwrap()
                    .feedback
                    .key,
                ImsPcbKeyFeedbackV1::Unsupported(Unproved::LogicalRelationship)
            );
            let inserted = call(
                service,
                run,
                feedback(run, 4, ImsOperation::Insert, 1, &["ROOT"], b"C3Z"),
            )
            .unwrap();
            assert_eq!(inserted.result.status, "  ");
            assert_eq!(
                inserted.feedback.key,
                ImsPcbKeyFeedbackV1::Unsupported(Unproved::LogicalRelationship)
            );
        }
    }
}

#[test]
fn logical_navigation_g_ap_class_does_not_promote_go_feedback() {
    for options in ["G", "GO"] {
        for store in failure_tests::backends() {
            let run = "logical-options";
            // The fixture's real insertion uses PCB 1's AP authority; the
            // independently scheduled read uses PCB 3's declared options.
            let mut m = logical_catalog();
            let ImsPcbMetadata::Database(pcb) = &mut m.psbs[0].pcbs[2] else {
                unreachable!()
            };
            pcb.processing_options = options.into();
            let service = seed_catalog(store, run, m);
            let got = call(
                service.clone(),
                run,
                feedback(run, 2, ImsOperation::GetUnique, 3, &["CHILD"], b""),
            )
            .unwrap();
            assert_eq!(got.result.status, "  ");
            assert_eq!(got.result.segments[0].data, b"C1ZL2X");
            assert_source_position(&service, run, 3, &[b"A1X", b"C1Z"]);
            if options == "G" {
                assert_key(&got, "CHILD", 2, b"A1C1", 6);
            } else {
                assert_eq!(
                    got.feedback.key,
                    ImsPcbKeyFeedbackV1::Unsupported(Unproved::LogicalRelationship)
                );
            }
        }
    }
}

#[test]
fn logical_navigation_root_link_without_physical_child_ancestry_stays_unproved() {
    for store in failure_tests::backends() {
        let run = "logical-root-link";
        let service = ImsService::open(store, Default::default()).unwrap();
        let mut m = logical_catalog();
        m.databases[0].logical_relationships[0].child_segment = "ROOT".into();
        service.install_metadata(m).unwrap();
        assert_eq!(
            execute(
                &service,
                run,
                &request(run, ImsOperation::Schedule, 1, &[], b"")
            )
            .status,
            "  "
        );
        for (seq, pcb, name, data, qualifiers) in [
            (2, 2, "DROOT", b"P9A".as_slice(), vec![]),
            (3, 2, "LPARENT", b"L2X", vec![q("DROOT", "DROOTKEY", b"P9")]),
            (4, 1, "ROOT", b"A1X", vec![q("LPARENT", "KIND", b"X")]),
        ] {
            let mut r = request(run, ImsOperation::Insert, seq, &[name], data);
            r.pcb = pcb;
            r.qualifiers = qualifiers;
            assert_eq!(execute(&service, run, &r).status, "  ");
        }
        assert_eq!(
            execute(
                &service,
                run,
                &request(run, ImsOperation::Commit, 5, &[], b"")
            )
            .status,
            "  "
        );
        let got = call(
            service.clone(),
            run,
            feedback(run, 6, ImsOperation::GetUnique, 1, &["ROOT"], b""),
        )
        .unwrap();
        assert_eq!(got.result.status, "  ");
        assert_eq!(got.result.segments[0].data, b"A1XL2X");
        assert_eq!(got.feedback.transferred_data_length, 6);
        assert_eq!(
            got.feedback.key,
            ImsPcbKeyFeedbackV1::Unsupported(Unproved::LogicalRelationship)
        );
        assert_source_position(&service, run, 1, &[b"A1X"]);
        // This models an existing owned metadata shape, not IBM root logical
        // execution credit. The leaf cannot infer its missing ancestry recipe.
    }
}

fn rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    [
        STATE_NAMESPACE,
        GENERIC_DATABASE_NAMESPACE,
        GENERIC_PENDING_NAMESPACE,
        SESSION_NAMESPACE,
        REPLAY_NAMESPACE,
        SYSTEM_NAMESPACE,
    ]
    .into_iter()
    .flat_map(|ns| store.list_provider_state(ns, 4096).unwrap())
    .collect()
}

#[test]
fn logical_navigation_capacity_session_cas_lost_ack_replay_and_reopen() {
    for store in failure_tests::backends() {
        let run = "logical-atomic";
        let service = seed_logical(store.clone(), run);
        let mut req = child_c(run, 2, 1, b"A1C1");
        req.key_capacity = 3;
        let before = rows(&*store);
        assert_eq!(
            call(service.clone(), run, req.clone()),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(rows(&*store), before);
        let before_position = snapshot(&service);
        assert_eq!(
            call(service.clone(), run, req.clone()),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(snapshot(&service), before_position);
        req.key_capacity = 4;
        let cas = super::super::session_cas::SessionCasStore::new(store.clone(), run);
        let stale = ImsService::open(cas.clone(), Default::default()).unwrap();
        let db_before = store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
            .unwrap();
        cas.arm();
        assert_eq!(
            call(stale, run, req.clone()),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(
            store
                .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
                .unwrap(),
            db_before
        );
        assert!(
            store
                .get_provider_state(REPLAY_NAMESPACE, "logical-atomic-2")
                .unwrap()
                .is_none()
        );
        let service = ImsService::open(cas.clone(), Default::default()).unwrap();
        cas.lose_ack();
        assert_eq!(
            call(service, run, req.clone()),
            Err(HostProblem::UnknownOutcome)
        );
        let service = ImsService::open(store.clone(), Default::default()).unwrap();
        let retained = call(service.clone(), run, req.clone()).unwrap();
        assert_key(&retained, "CHILD", 2, b"A1C1", 6);
        assert_eq!(retained.result.segments[0].data, b"C1ZL2X");
        call(
            service.clone(),
            run,
            feedback(run, 3, ImsOperation::GetHoldUnique, 1, &["CHILD"], b""),
        )
        .unwrap();
        assert_eq!(
            call(
                service.clone(),
                run,
                feedback(run, 4, ImsOperation::Replace, 1, &[], b"C1Q")
            )
            .unwrap()
            .result
            .status,
            "  "
        );
        execute(
            &service,
            run,
            &request(run, ImsOperation::Commit, 5, &[], b""),
        );
        let before = rows(&*store);
        assert_eq!(call(service.clone(), run, req.clone()).unwrap(), retained);
        assert_eq!(rows(&*store), before);
        let mut conflict = req.clone();
        conflict.key_capacity = 5;
        assert_eq!(
            call(service, run, conflict),
            Err(HostProblem::IdempotencyConflict)
        );
        let reopened = ImsService::open(store, Default::default()).unwrap();
        assert_eq!(call(reopened.clone(), run, req).unwrap(), retained);
        let current = call(reopened, run, child_c(run, 6, 1, b"A1C1")).unwrap();
        assert_key(&current, "CHILD", 2, b"A1C1", 6);
        assert_eq!(current.result.segments[0].data, b"C1QL2X");
    }
}

struct LogicalPolicy(std::sync::atomic::AtomicU8);
impl EnterpriseAuthorizer for LogicalPolicy {
    fn authorize(&self, _: &PrincipalId, resource: &EnterpriseResource) -> Result<(), HostProblem> {
        let mode = self.0.load(Ordering::SeqCst);
        if resource.class == EnterpriseResourceClass::ImsDatabase
            && ((mode == 1 && resource.name.as_str() == "GENDB")
                || (mode == 2 && resource.name.as_str() == "PARENTDB"))
        {
            return Err(HostProblem::Unauthorized);
        }
        if mode == 3 && resource.class == EnterpriseResourceClass::ImsDatabase {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(())
    }
}

#[test]
fn logical_navigation_source_destination_saf_foreign_undo_q_and_integrity() {
    for store in failure_tests::backends() {
        let run = "logical-safety";
        drop(seed_logical(store.clone(), run));
        let policy = Arc::new(LogicalPolicy(std::sync::atomic::AtomicU8::new(0)));
        let service =
            ImsService::open_authorized(store.clone(), Default::default(), policy.clone()).unwrap();
        let req = child_c(run, 2, 1, b"A1C1");
        let saved = call(service.clone(), run, req.clone()).unwrap();
        assert_key(&saved, "CHILD", 2, b"A1C1", 6);
        for mode in [1, 2, 3] {
            policy.0.store(mode, Ordering::SeqCst);
            let error = if mode == 3 {
                HostProblem::InfrastructureFailure
            } else {
                HostProblem::Unauthorized
            };
            let before = rows(&*store);
            assert_eq!(call(service.clone(), run, req.clone()), Err(error.clone()));
            assert_eq!(
                call(service.clone(), run, child_c(run, 3, 4, b"A1C1")),
                Err(error)
            );
            assert_eq!(rows(&*store), before);
        }
        policy.0.store(0, Ordering::SeqCst);
        let foreign = "logical-foreign";
        execute(
            &service,
            foreign,
            &request(foreign, ImsOperation::Schedule, 1, &[], b""),
        );
        let mut hold = feedback(
            foreign,
            2,
            ImsOperation::GetHoldUnique,
            2,
            &["LPARENT"],
            b"",
        );
        hold.request.qualifiers.push(q("LPARENT", "KIND", b"X"));
        assert_eq!(
            call(service.clone(), foreign, hold)
                .unwrap()
                .result
                .segments[0]
                .data,
            b"L2X"
        );
        assert_eq!(
            call(
                service.clone(),
                foreign,
                feedback(foreign, 3, ImsOperation::Replace, 2, &[], b"L2R")
            )
            .unwrap()
            .result
            .status,
            "  "
        );
        let before = rows(&*store);
        assert_eq!(
            call(service.clone(), run, child_c(run, 3, 1, b"A1C1")),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(rows(&*store), before);
        execute(
            &service,
            foreign,
            &request(foreign, ImsOperation::Rollback, 4, &[], b""),
        );
        // Q belongs to the typed request owner; the SSA route forbids Q.
        let mut reserve = feedback(run, 3, ImsOperation::GetUnique, 1, &["CHILD"], b"");
        reserve.request.qualifiers = vec![qualifier(b"A1"), q("CHILD", "CHILDKEY", b"C1")];
        reserve.request.q_class = ImsQClass::new(b'A');
        assert_key(
            &call(service.clone(), run, reserve).unwrap(),
            "CHILD",
            2,
            b"A1C1",
            6,
        );
        call(
            service.clone(),
            foreign,
            feedback(
                foreign,
                5,
                ImsOperation::GetHoldUnique,
                2,
                &["LPARENT"],
                b"",
            ),
        )
        .unwrap();
        let before = rows(&*store);
        assert_eq!(
            call(
                service.clone(),
                foreign,
                feedback(foreign, 6, ImsOperation::Delete, 2, &[], b"")
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(rows(&*store), before);
        execute(
            &service,
            run,
            &request(run, ImsOperation::Commit, 4, &[], b""),
        );
        let deleted = call(
            service.clone(),
            foreign,
            feedback(foreign, 6, ImsOperation::Delete, 2, &[], b""),
        )
        .unwrap();
        assert_eq!(deleted.result.status, "  ");
        assert_eq!(deleted.result.affected_segments, 2);
        execute(
            &service,
            foreign,
            &request(foreign, ImsOperation::Rollback, 7, &[], b""),
        );
        assert_eq!(call(service.clone(), run, req).unwrap(), saved);
        // Remove the exact destination occurrence using a valid engine image.
        // Existing cross-image integrity, before even K projection, must fail.
        let mut row = store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "PARENTDB")
            .unwrap()
            .unwrap();
        let mut json: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        let mut engine = DatabaseEngine::restore(
            serde_json::from_value(json["value"].clone()).unwrap(),
            engine_limits(Default::default()),
        )
        .unwrap();
        let id = engine
            .export_records()
            .into_iter()
            .find(|v| v.data == b"L2X")
            .unwrap()
            .id;
        engine.delete_by_id(id).unwrap();
        json["value"] = serde_json::to_value(engine.image()).unwrap();
        let prior = row.version;
        row.version += 1;
        row.payload = serde_json::to_vec(&json).unwrap();
        store.put_provider_state(row, Some(prior)).unwrap();
        let before = rows(&*store);
        assert_eq!(
            call(service, run, child_c(run, 5, 4, b"A1C1")),
            Err(HostProblem::InfrastructureFailure)
        );
        assert_eq!(rows(&*store), before);
    }
}

#[test]
fn logical_navigation_sqlite_substantive_separate_process() {
    const PHASE: &str = "IMS_LOGICAL_FEEDBACK_PHASE";
    const URL: &str = "IMS_LOGICAL_FEEDBACK_URL";
    if let Ok(phase) = std::env::var(PHASE) {
        let store = Arc::new(
            SqliteStateStore::open(&std::env::var(URL).unwrap(), 64 * 1024 * 1024, 262144).unwrap(),
        );
        let run = "logical-process";
        let service = if phase == "seed" {
            seed_logical(store, run)
        } else {
            ImsService::open(store, Default::default()).unwrap()
        };
        let old = call(service.clone(), run, child_c(run, 2, 1, b"A1C1")).unwrap();
        assert_key(&old, "CHILD", 2, b"A1C1", 6);
        assert_eq!(old.result.segments[0].data, b"C1ZL2X");
        if phase == "mutate" {
            call(
                service.clone(),
                run,
                feedback(run, 3, ImsOperation::GetHoldUnique, 1, &["CHILD"], b""),
            )
            .unwrap();
            assert_eq!(
                call(
                    service.clone(),
                    run,
                    feedback(run, 4, ImsOperation::Replace, 1, &[], b"C1Q")
                )
                .unwrap()
                .result
                .status,
                "  "
            );
            execute(
                &service,
                run,
                &request(run, ImsOperation::Commit, 5, &[], b""),
            );
        } else if phase == "reopen" {
            let current = call(service, run, child_c(run, 6, 1, b"A1C1")).unwrap();
            assert_key(&current, "CHILD", 2, b"A1C1", 6);
            assert_eq!(current.result.segments[0].data, b"C1QL2X");
        }
        return;
    }
    let file = std::env::temp_dir().join(format!(
        "logical-feedback-process-{}.sqlite",
        std::process::id()
    ));
    let url = format!("sqlite:{}?mode=rwc", file.display());
    for phase in ["seed", "mutate", "reopen"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "service::generic::tests::feedback_tests::logical_navigation_tests::logical_navigation_sqlite_substantive_separate_process", "--nocapture"])
            .env(PHASE, phase).env(URL, &url).output().unwrap();
        assert!(
            output.status.success(),
            "{phase}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        println!("logical process {phase}: 1 substantive child passed");
    }
    std::fs::remove_file(file).unwrap();
}

#[test]
fn logical_navigation_historical_unsupported_receipt_replays_without_reprojection() {
    for store in failure_tests::backends() {
        let run = "logical-old";
        let service = seed_logical(store.clone(), run);
        let req = child_c(run, 2, 1, b"A1C1");
        // Independent base-shape receipt. No fresh handler output supplies the
        // expected key, data, parent key, transfer length or availability.
        let old = ImsPcbFeedbackResultV1 {
            result: ImsResult {
                status: "  ".into(),
                segments: vec![ImsSegment {
                    name: "CHILD".into(),
                    parent_key: Some(b"A1".to_vec()),
                    data: b"C1ZL2X".to_vec(),
                }],
                checkpoint_id: None,
                affected_segments: 0,
                system: None,
            },
            feedback: mainframe_env_host_api::ImsPcbFeedbackV1 {
                pcb: 1,
                database: "GENDB".into(),
                processing_options: "AP".into(),
                sensitive_segment_count: 2,
                transferred_data_length: 6,
                key: ImsPcbKeyFeedbackV1::Unsupported(Unproved::LogicalRelationship),
            },
        };
        let mut recorded = RecordedResult::from_result(
            mainframe_env_host_api::canonical_request_digest(&HostRequest::ImsPcbFeedbackV1(
                req.clone(),
            ))
            .unwrap(),
            &old.result,
        );
        recorded.pcb_feedback_v1 = Some(old.feedback.clone());
        prepare_ims_replay(
            &mut recorded,
            "logical-old-2",
            &invocation_class(run, ServiceClass::Batch),
            2,
            Default::default(),
        )
        .unwrap();
        resolve_ims_replay(&mut recorded, "logical-old-2", 100, 100).unwrap();
        let historical = ProviderStateRecord {
            namespace: REPLAY_NAMESPACE.into(),
            key: "logical-old-2".into(),
            version: 1,
            payload: encode_object_row("logical-old-2", &recorded).unwrap(),
        };
        store.put_provider_state(historical.clone(), None).unwrap();
        let before = generic::pcb::position(&service.lock().unwrap().state.sessions[run], 1);
        assert_eq!(call(service.clone(), run, req.clone()).unwrap(), old);
        assert_eq!(
            generic::pcb::position(&service.lock().unwrap().state.sessions[run], 1),
            before
        );
        assert_eq!(
            store
                .get_provider_state(REPLAY_NAMESPACE, "logical-old-2")
                .unwrap(),
            Some(historical.clone())
        );
        let mut fresh = child_c(run, 3, 1, b"A1C1");
        fresh.request.operation = ImsOperation::GetHoldUnique;
        assert_key(
            &call(service.clone(), run, fresh).unwrap(),
            "CHILD",
            2,
            b"A1C1",
            6,
        );
        assert_eq!(
            call(
                service.clone(),
                run,
                feedback(run, 4, ImsOperation::Replace, 1, &[], b"C1Q")
            )
            .unwrap()
            .result
            .status,
            "  "
        );
        execute(
            &service,
            run,
            &request(run, ImsOperation::Commit, 5, &[], b""),
        );
        drop(service);
        let reopened = ImsService::open(store.clone(), Default::default()).unwrap();
        assert_eq!(call(reopened, run, req).unwrap(), old);
        assert_eq!(
            store
                .get_provider_state(REPLAY_NAMESPACE, "logical-old-2")
                .unwrap(),
            Some(historical)
        );
    }
}

struct CapacityStore {
    inner: Arc<dyn ProviderStateStore>,
    reject: std::sync::atomic::AtomicBool,
}
impl mainframe_env_store_api::AuditSink for CapacityStore {
    fn record_audit(&self, r: mainframe_env_execution_api::AuditRecord) -> Result<(), StoreError> {
        self.inner.record_audit(r)
    }
    fn audit_records(
        &self,
        e: &ExecutionId,
        start: u64,
        max: usize,
    ) -> Result<Vec<mainframe_env_execution_api::AuditRecord>, StoreError> {
        self.inner.audit_records(e, start, max)
    }
}
impl ProviderStateStore for CapacityStore {
    fn advance_logical_clock(&self, floor: u64) -> Result<u64, StoreError> {
        self.inner.advance_logical_clock(floor)
    }
    fn get_provider_state(
        &self,
        ns: &str,
        key: &str,
    ) -> Result<Option<ProviderStateRecord>, StoreError> {
        self.inner.get_provider_state(ns, key)
    }
    fn list_provider_state(
        &self,
        ns: &str,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        self.inner.list_provider_state(ns, max)
    }
    fn put_provider_state(
        &self,
        r: ProviderStateRecord,
        expected: Option<u64>,
    ) -> Result<(), StoreError> {
        self.inner.put_provider_state(r, expected)
    }
    fn delete_provider_state(&self, ns: &str, key: &str, expected: u64) -> Result<(), StoreError> {
        self.inner.delete_provider_state(ns, key, expected)
    }
    fn move_provider_state(
        &self,
        r: ProviderStateRecord,
        key: &str,
        expected: u64,
    ) -> Result<(), StoreError> {
        self.inner.move_provider_state(r, key, expected)
    }
    fn put_provider_states_atomic(&self, w: Vec<ProviderStateWrite>) -> Result<(), StoreError> {
        self.inner.put_provider_states_atomic(w)
    }
    fn mutate_provider_states_atomic(
        &self,
        m: Vec<ProviderStateMutation>,
    ) -> Result<(), StoreError> {
        if self.reject.swap(false, Ordering::SeqCst) {
            return Err(StoreError::CapacityExceeded);
        }
        self.inner.mutate_provider_states_atomic(m)
    }
}

#[test]
fn logical_navigation_atomic_store_capacity_discards_key_and_position_proposal() {
    for inner in failure_tests::backends() {
        let run = "logical-store-full";
        let store = Arc::new(CapacityStore {
            inner,
            reject: std::sync::atomic::AtomicBool::new(false),
        });
        let service = seed_logical(store.clone(), run);
        let before = rows(&*store);
        let req = child_c(run, 2, 1, b"A1C1");
        store.reject.store(true, Ordering::SeqCst);
        assert_eq!(
            call(service.clone(), run, req.clone()),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(rows(&*store), before);
        assert_eq!(
            generic::pcb::position(&service.lock().unwrap().state.sessions[run], 1),
            PcbPosition::default()
        );
        assert_key(&call(service, run, req).unwrap(), "CHILD", 2, b"A1C1", 6);
    }
}
