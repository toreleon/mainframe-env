use super::*;
use mainframe_env_host_api::{
    HostLimits, ImsPcbFeedbackRequestV1, ImsPcbFeedbackResultV1,
    ImsPcbFeedbackUnsupportedV1 as Unproved, ImsPcbKeyFeedbackV1,
};

mod failure_tests;

fn snapshot(service: &ImsService) -> (Vec<u8>, RowVersions) {
    let d = service.lock().unwrap();
    (serde_json::to_vec(&d.state).unwrap(), d.versions.clone())
}

fn seed(
    store: Arc<dyn ProviderStateStore>,
    run: &str,
    key_only: bool,
    secondary: bool,
) -> Arc<ImsService> {
    let mut metadata = catalog();
    let ImsPcbMetadata::Database(mut pcb) = metadata.psbs[0].pcbs[0].clone() else {
        unreachable!()
    };
    pcb.name = "SECOND".into();
    if key_only {
        pcb.sensitive_segments[1].processing_options = Some("K".into());
    }
    if secondary {
        metadata.databases[0]
            .secondary_indexes
            .push(ImsSecondaryIndexMetadata {
                name: "BYCHILD".into(),
                source_segment: "CHILD".into(),
                target_segment: "ROOT".into(),
                source_fields: vec!["KIND".into()],
            });
        pcb.secondary_index = Some("BYCHILD".into());
    }
    metadata.psbs[0].pcbs.push(ImsPcbMetadata::Database(pcb));
    let service = ImsService::open(store, Default::default()).unwrap();
    service.install_metadata(metadata).unwrap();
    let image = ImsGenericLoadImage {
        database: "GENDB".into(),
        records: vec![
            ImsGenericLoadRecord {
                segment: "ROOT".into(),
                parent: None,
                data: b"A1X".to_vec(),
            },
            ImsGenericLoadRecord {
                segment: "CHILD".into(),
                parent: Some(0),
                data: b"C1Z".to_vec(),
            },
            ImsGenericLoadRecord {
                segment: "ROOT".into(),
                parent: None,
                data: b"B2Y".to_vec(),
            },
            ImsGenericLoadRecord {
                segment: "CHILD".into(),
                parent: Some(2),
                data: b"D2A".to_vec(),
            },
        ],
    };
    execute(
        &service,
        "feedback-seed",
        &request(
            "feedback-seed",
            ImsOperation::Load,
            1,
            &[],
            &serde_json::to_vec(&image).unwrap(),
        ),
    );
    execute(
        &service,
        "feedback-seed",
        &request("feedback-seed", ImsOperation::Commit, 2, &[], b""),
    );
    execute(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    service
}

fn assert_key(result: &ImsPcbFeedbackResultV1, name: &str, level: u16, key: &[u8], length: u64) {
    assert_eq!(result.result.status, "  ");
    assert_eq!(
        result.feedback.key,
        ImsPcbKeyFeedbackV1::Valid {
            segment_name: name.into(),
            segment_level: level,
            bytes: key.to_vec(),
        }
    );
    assert_eq!(result.feedback.key.valid_length(), Some(key.len()));
    assert_eq!(result.feedback.transferred_data_length, length);
}

#[test]
fn public_feedback_all_six_gets_and_mutation_statuses_keep_existing_owners() {
    for store in failure_tests::backends() {
        let run = "feedback-six-gets";
        let service = seed(store, run, false, false);
        let mut seq = 2;
        for op in [
            ImsOperation::GetUnique,
            ImsOperation::GetHoldUnique,
            ImsOperation::GetNext,
            ImsOperation::GetHoldNext,
            ImsOperation::GetNextParent,
            ImsOperation::GetHoldNextParent,
        ] {
            public(
                service.clone(),
                run,
                feedback(run, seq, ImsOperation::GetUnique, 1, &["ROOT"], b""),
            )
            .unwrap();
            seq += 1;
            let found = public(
                service.clone(),
                run,
                feedback(run, seq, op, 1, &["CHILD"], b""),
            )
            .unwrap();
            assert_key(&found, "CHILD", 2, b"A1C1", 3);
            seq += 1;
            // Re-establish parentage before testing the unsuccessful current call.
            public(
                service.clone(),
                run,
                feedback(run, seq, ImsOperation::GetUnique, 1, &["ROOT"], b""),
            )
            .unwrap();
            seq += 1;
            let mut missing = feedback(run, seq, op, 1, &["CHILD"], b"");
            missing.request.qualifiers.push(ImsQualifier {
                segment: "CHILD".into(),
                field: "CHILDKEY".into(),
                value: b"NO".to_vec(),
            });
            let failed = public(service.clone(), run, missing).unwrap();
            assert_eq!(failed.result.status, "GE");
            assert_eq!(failed.feedback.transferred_data_length, 0);
            assert_eq!(
                failed.feedback.key,
                ImsPcbKeyFeedbackV1::Unsupported(Unproved::FailedCallWitness)
            );
            seq += 1;
        }
        let duplicate = public(
            service.clone(),
            run,
            feedback(run, seq, ImsOperation::Insert, 1, &["ROOT"], b"A1T"),
        )
        .unwrap();
        assert_eq!(duplicate.result.status, "II");
        assert_eq!(
            duplicate.feedback.key,
            ImsPcbKeyFeedbackV1::Unsupported(Unproved::FailedCallWitness)
        );
        seq += 1;
        public(
            service.clone(),
            run,
            feedback(run, seq, ImsOperation::GetHoldUnique, 1, &["CHILD"], b""),
        )
        .unwrap();
        seq += 1;
        let changed_key = public(
            service.clone(),
            run,
            feedback(run, seq, ImsOperation::Replace, 1, &[], b"C9Z"),
        )
        .unwrap();
        assert_eq!(changed_key.result.status, "DA");
        assert_eq!(
            changed_key.feedback.key,
            ImsPcbKeyFeedbackV1::Unsupported(Unproved::FailedCallWitness)
        );
        seq += 1;
        public(
            service.clone(),
            run,
            feedback(run, seq, ImsOperation::GetHoldUnique, 1, &["CHILD"], b""),
        )
        .unwrap();
        seq += 1;
        let deleted = public(
            service.clone(),
            run,
            feedback(run, seq, ImsOperation::Delete, 1, &[], b""),
        )
        .unwrap();
        assert_eq!(deleted.result.status, "  ");
        assert_eq!(deleted.result.affected_segments, 1);
        assert_eq!(deleted.feedback.transferred_data_length, 0);
        assert_eq!(
            deleted.feedback.key,
            ImsPcbKeyFeedbackV1::Unsupported(Unproved::NonKeyOperation)
        );
        seq += 1;
        let no_hold = public(
            service.clone(),
            run,
            feedback(run, seq, ImsOperation::Delete, 1, &[], b""),
        )
        .unwrap();
        assert_eq!(no_hold.result.status, "DJ");
        assert_eq!(
            no_hold.feedback.key,
            ImsPcbKeyFeedbackV1::Unsupported(Unproved::FailedCallWitness)
        );
    }
}

#[test]
fn public_feedback_navigation_failure_and_independent_pcbs_memory_and_sqlite() {
    for store in failure_tests::backends() {
        let run = "feedback-navigation";
        let service = seed(store.clone(), run, false, false);
        let gp = public(
            service.clone(),
            run,
            feedback(run, 2, ImsOperation::GetHoldNextParent, 1, &["CHILD"], b""),
        )
        .unwrap();
        assert_eq!(gp.result.status, "GP");
        assert_eq!(
            gp.feedback.key,
            ImsPcbKeyFeedbackV1::Unsupported(Unproved::FailedCallWitness)
        );
        assert_eq!(gp.feedback.transferred_data_length, 0);
        assert_eq!(
            generic::pcb::position(&service.lock().unwrap().state.sessions[run], 1),
            PcbPosition::default()
        );
        let first = feedback(run, 3, ImsOperation::GetHoldUnique, 1, &["ROOT"], b"");
        let got = public(service.clone(), run, first.clone()).unwrap();
        assert_key(&got, "ROOT", 1, b"A1", 3);
        let held = generic::pcb::position(&service.lock().unwrap().state.sessions[run], 1);
        assert!(held.is_held());
        let mut other = feedback(run, 4, ImsOperation::GetHoldUnique, 2, &["ROOT"], b"");
        other.request.qualifiers = vec![qualifier(b"B2")];
        assert_key(
            &public(service.clone(), run, other).unwrap(),
            "ROOT",
            1,
            b"B2",
            3,
        );
        assert_eq!(
            generic::pcb::position(&service.lock().unwrap().state.sessions[run], 1),
            held
        );
        let child = public(
            service.clone(),
            run,
            feedback(run, 5, ImsOperation::GetHoldNextParent, 1, &["CHILD"], b""),
        )
        .unwrap();
        assert_key(&child, "CHILD", 2, b"A1C1", 3);
        let position = generic::pcb::position(&service.lock().unwrap().state.sessions[run], 1);
        assert!(position.is_held());
        let mut missing = feedback(run, 6, ImsOperation::GetHoldUnique, 1, &["CHILD"], b"");
        missing.request.qualifiers.push(ImsQualifier {
            segment: "CHILD".into(),
            field: "CHILDKEY".into(),
            value: b"NO".to_vec(),
        });
        let failed = public(service.clone(), run, missing).unwrap();
        assert_eq!(failed.result.status, "GE");
        assert_eq!(
            failed.feedback.key,
            ImsPcbKeyFeedbackV1::Unsupported(Unproved::FailedCallWitness)
        );
        let failed_position =
            generic::pcb::position(&service.lock().unwrap().state.sessions[run], 1);
        assert_eq!(failed_position.current(), position.current());
        assert_eq!(failed_position.parentage(), None);
        assert!(!failed_position.is_held());
        let lost = public(
            service.clone(),
            run,
            feedback(run, 7, ImsOperation::Replace, 1, &[], b"C1Q"),
        )
        .unwrap();
        assert_eq!(lost.result.status, "DJ");
        assert_eq!(
            lost.feedback.key,
            ImsPcbKeyFeedbackV1::Unsupported(Unproved::FailedCallWitness)
        );
        public(
            service.clone(),
            run,
            feedback(run, 8, ImsOperation::GetHoldUnique, 1, &["CHILD"], b""),
        )
        .unwrap();
        let replaced = public(
            service.clone(),
            run,
            feedback(run, 9, ImsOperation::Replace, 1, &[], b"C1Q"),
        )
        .unwrap();
        assert_eq!(replaced.result.status, "  ");
        assert_eq!(
            replaced.feedback.key,
            ImsPcbKeyFeedbackV1::Unsupported(Unproved::NonKeyOperation)
        );
        execute(
            &service,
            run,
            &request(run, ImsOperation::Commit, 10, &[], b""),
        );
        let next = public(
            service.clone(),
            run,
            feedback(run, 11, ImsOperation::GetHoldNext, 1, &["ROOT"], b""),
        )
        .unwrap();
        assert_key(&next, "ROOT", 1, b"B2", 3);
        let eof = public(
            service.clone(),
            run,
            feedback(run, 12, ImsOperation::GetNext, 1, &["ROOT"], b""),
        )
        .unwrap();
        assert_eq!(eof.result.status, "GE");
        assert!(
            generic::pcb::position(&service.lock().unwrap().state.sessions[run], 1)
                .current()
                .is_none()
        );
        for (seq, name, level, key) in [
            (13, "ROOT", 1, b"A1".as_slice()),
            (14, "CHILD", 2, b"A1C1".as_slice()),
            (15, "ROOT", 1, b"B2".as_slice()),
            (16, "CHILD", 2, b"B2D2".as_slice()),
        ] {
            assert_key(
                &public(
                    service.clone(),
                    run,
                    feedback(run, seq, ImsOperation::GetNext, 1, &[], b""),
                )
                .unwrap(),
                name,
                level,
                key,
                3,
            );
        }
        let eof = public(
            service.clone(),
            run,
            feedback(run, 17, ImsOperation::GetNext, 1, &[], b""),
        )
        .unwrap();
        assert_eq!(eof.result.status, "GB");
        assert_eq!(
            eof.feedback.key,
            ImsPcbKeyFeedbackV1::Unsupported(Unproved::FailedCallWitness)
        );
        let before = snapshot(&service);
        assert_eq!(public(service.clone(), run, first.clone()).unwrap(), got);
        assert_eq!(snapshot(&service), before);
        drop(service);
        let reopened = ImsService::open(store, Default::default()).unwrap();
        assert_eq!(public(reopened, run, first).unwrap(), got);
    }
}

#[test]
fn public_feedback_ssa_path_and_key_only_use_selected_occurrence_without_data_leak() {
    for store in failure_tests::backends() {
        let run = "feedback-key-only";
        let service = seed(store, run, true, false);
        let mut path = feedback(run, 2, ImsOperation::GetUnique, 1, &[], b"");
        path.ssas = Some(vec![b"ROOT    *D ".to_vec(), b"CHILD    ".to_vec()]);
        let all = public(service.clone(), run, path.clone()).unwrap();
        assert_key(&all, "CHILD", 2, b"A1C1", 6);
        assert_eq!(
            all.result
                .segments
                .iter()
                .map(|s| s.data.clone())
                .collect::<Vec<_>>(),
            [b"A1X".to_vec(), b"C1Z".to_vec()]
        );
        path.request.pcb = 2;
        path.request.mutation = request(run, ImsOperation::GetUnique, 3, &[], b"").mutation;
        let hidden = public(service.clone(), run, path).unwrap();
        assert_key(&hidden, "CHILD", 2, b"A1C1", 3);
        assert_eq!(hidden.result.segments.len(), 1);
        assert_eq!(hidden.result.segments[0].data, b"A1X");
        let only = public(
            service.clone(),
            run,
            feedback(run, 4, ImsOperation::GetUnique, 2, &["CHILD"], b""),
        )
        .unwrap();
        assert_key(&only, "CHILD", 2, b"A1C1", 0);
        assert!(only.result.segments.is_empty());
        let next = public(
            service.clone(),
            run,
            feedback(run, 5, ImsOperation::GetNext, 2, &[], b""),
        )
        .unwrap();
        assert_key(&next, "ROOT", 1, b"B2", 3);
    }
}

#[test]
fn public_feedback_secondary_target_and_replace_invalidation_do_not_guess_index_keys() {
    for store in failure_tests::backends() {
        let run = "feedback-secondary";
        let service = seed(store, run, false, true);
        let got = public(
            service.clone(),
            run,
            feedback(run, 2, ImsOperation::GetHoldUnique, 2, &["ROOT"], b""),
        )
        .unwrap();
        // Secondary source D2A has index A; the returned ancestor target is B2Y.
        assert_eq!(got.result.segments[0].data, b"B2Y");
        assert_eq!(
            got.feedback.key,
            ImsPcbKeyFeedbackV1::Unsupported(Unproved::SecondarySequence)
        );
        assert_eq!(got.feedback.key.valid_length(), None);
        let repl = public(
            service.clone(),
            run,
            feedback(run, 3, ImsOperation::Replace, 2, &[], b"B2Q"),
        )
        .unwrap();
        assert_eq!(repl.result.status, "  ");
        assert_eq!(
            repl.feedback.key,
            ImsPcbKeyFeedbackV1::InvalidatedSecondaryReplace
        );
        // Hold and parentage remain the navigation owner's behavior. A later
        // non-hold Get cancels the hold; feedback must not recreate it.
        public(
            service.clone(),
            run,
            feedback(run, 4, ImsOperation::GetUnique, 2, &["ROOT"], b""),
        )
        .unwrap();
        let lost = public(
            service.clone(),
            run,
            feedback(run, 5, ImsOperation::Replace, 2, &[], b"B2T"),
        )
        .unwrap();
        assert_eq!(lost.result.status, "DJ");
        assert_eq!(
            lost.feedback.key,
            ImsPcbKeyFeedbackV1::Unsupported(Unproved::FailedCallWitness)
        );
    }
}

#[test]
fn public_feedback_capacity_conflict_and_forbidden_shapes_publish_nothing() {
    for store in failure_tests::backends() {
        let run = "feedback-capacity";
        let service = seed(store, run, false, false);
        let mut req = feedback(run, 2, ImsOperation::Insert, 1, &["ROOT"], b"C3Z");
        req.key_capacity = 1;
        let before = snapshot(&service);
        assert_eq!(
            public(service.clone(), run, req.clone()),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(snapshot(&service), before);
        req.key_capacity = 2;
        let inserted = public(service.clone(), run, req.clone()).unwrap();
        assert_key(&inserted, "ROOT", 1, b"C3", 0);
        req.key_capacity = 3;
        let before = snapshot(&service);
        assert_eq!(
            public(service.clone(), run, req.clone()),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(snapshot(&service), before);
        req.context = ImsExecutionContext::TmBatch;
        assert_eq!(
            public(service.clone(), run, req),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(snapshot(&service), before);
        let malformed = feedback(run, 3, ImsOperation::GetUnique, 1, &["ROOT"], b"ignored");
        assert_eq!(
            public(service.clone(), run, malformed),
            Err(HostProblem::Malformed)
        );
        let malformed = feedback(run, 3, ImsOperation::Replace, 1, &["ROOT"], b"C3Q");
        assert_eq!(
            public(service.clone(), run, malformed),
            Err(HostProblem::Malformed)
        );
        assert_eq!(snapshot(&service), before);
        let mut ssa = feedback(run, 3, ImsOperation::GetUnique, 1, &[], b"");
        ssa.ssas = Some(vec![b"ROOT    (UNKNOWN EQAA)".to_vec()]);
        assert_eq!(
            public(service.clone(), run, ssa),
            Err(HostProblem::Malformed)
        );
        assert_eq!(snapshot(&service), before);
    }
}

#[test]
fn public_feedback_file_sqlite_fresh_connection_retains_exact_reply_after_later_mutation() {
    let file = std::env::temp_dir().join(format!(
        "ims-feedback-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite:{}?mode=rwc", file.display());
    let run = "feedback-file";
    let first = feedback(run, 2, ImsOperation::GetUnique, 1, &["CHILD"], b"");
    let saved;
    {
        let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).unwrap());
        let service = seed(store, run, false, false);
        saved = public(service.clone(), run, first.clone()).unwrap();
        assert_key(&saved, "CHILD", 2, b"A1C1", 3);
        public(
            service.clone(),
            run,
            feedback(run, 3, ImsOperation::GetHoldUnique, 1, &["CHILD"], b""),
        )
        .unwrap();
        public(
            service.clone(),
            run,
            feedback(run, 4, ImsOperation::Replace, 1, &[], b"C1Q"),
        )
        .unwrap();
        execute(
            &service,
            run,
            &request(run, ImsOperation::Commit, 5, &[], b""),
        );
    }
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).unwrap());
    let service = ImsService::open(store, Default::default()).unwrap();
    assert_eq!(public(service.clone(), run, first).unwrap(), saved);
    assert_eq!(
        public(
            service,
            run,
            feedback(run, 6, ImsOperation::GetUnique, 1, &["CHILD"], b"")
        )
        .unwrap()
        .result
        .segments[0]
            .data,
        b"C1Q"
    );
    std::fs::remove_file(file).unwrap();
}

fn feedback(
    run: &str,
    seq: u64,
    op: ImsOperation,
    pcb: u16,
    segments: &[&str],
    data: &[u8],
) -> ImsPcbFeedbackRequestV1 {
    let mut request = request(run, op, seq, segments, data);
    request.pcb = pcb;
    ImsPcbFeedbackRequestV1 {
        request,
        context: ImsExecutionContext::DbBatch,
        ssas: None,
        key_capacity: 64,
    }
}

fn public(
    service: Arc<ImsService>,
    run: &str,
    request: ImsPcbFeedbackRequestV1,
) -> Result<ImsPcbFeedbackResultV1, HostProblem> {
    let request = HostRequest::ImsPcbFeedbackV1(request);
    request.validate(HostLimits::default())?;
    let provider = ims_providers(service, InvocationLimits::default()).remove(1);
    match provider
        .invoke(
            &invocation(run),
            EffectRequest {
                run_unit: invocation(run).run_unit_id.clone(),
                sequence: request.mutation().unwrap().sequence,
                deadline_tick: 100,
                idempotency_key: request.mutation().map(|m| m.idempotency_key.clone()),
                request,
            },
        )
        .outcome?
    {
        HostResult::ImsPcbFeedbackV1(result) => Ok(result),
        _ => panic!("missing owned PCB feedback result"),
    }
}

#[test]
fn public_owned_pcb_feedback_is_distinct_from_data_and_replays_after_later_work() {
    let run = "pcb-feedback";
    let store = Arc::new(MemoryStore::new(Default::default()));
    let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    service.install_metadata(catalog()).unwrap();
    execute(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    let first = feedback(run, 2, ImsOperation::Insert, 1, &["ROOT"], b"A1X");
    let inserted = public(service.clone(), run, first.clone()).unwrap();
    assert_eq!(inserted.feedback.database, "GENDB");
    assert_eq!(inserted.feedback.processing_options, "AP");
    assert_eq!(inserted.feedback.sensitive_segment_count, 2);
    assert_eq!(inserted.feedback.transferred_data_length, 0);
    assert_eq!(
        inserted.feedback.key,
        ImsPcbKeyFeedbackV1::Valid {
            segment_name: "ROOT".into(),
            segment_level: 1,
            bytes: b"A1".to_vec()
        }
    );
    let got = public(
        service.clone(),
        run,
        feedback(run, 3, ImsOperation::GetHoldUnique, 1, &["ROOT"], b""),
    )
    .unwrap();
    assert_eq!(got.result.segments[0].data, b"A1X");
    assert_eq!(got.feedback.transferred_data_length, 3);
    assert_eq!(got.feedback.key, inserted.feedback.key);
    public(
        service.clone(),
        run,
        feedback(run, 4, ImsOperation::Insert, 1, &["ROOT"], b"B2Y"),
    )
    .unwrap();
    assert_eq!(
        public(service.clone(), run, first.clone()).unwrap(),
        inserted
    );
    drop(service);
    let reopened = ImsService::open(store, ImsLimits::default()).unwrap();
    assert_eq!(public(reopened, run, first).unwrap(), inserted);
}
