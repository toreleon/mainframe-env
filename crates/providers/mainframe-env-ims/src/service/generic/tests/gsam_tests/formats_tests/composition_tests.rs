use super::*;
use mainframe_env_host_api::ImsPcbFeedbackRequestV1;

fn batch_public(service: Arc<ImsService>, run: &str, request: HostRequest) -> HostResult {
    let invocation = invocation_class(run, ServiceClass::Batch);
    ims_providers(service, InvocationLimits::default())
        .remove(1)
        .invoke(
            &invocation,
            EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence: request.mutation().unwrap().sequence,
                deadline_tick: invocation.deadline_tick,
                idempotency_key: request.mutation().map(|m| m.idempotency_key.clone()),
                request,
            },
        )
        .outcome
        .unwrap()
}

#[test]
fn gsam_formats_batch_and_feedback_keep_undo_until_explicit_boundary() {
    for kind in [ImsGsamRecordFormat::V, ImsGsamRecordFormat::U] {
        backends(&format!("composition-batch-{kind:?}"), |store| {
            let run = "format-batch";
            let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
            let mut c = metadata();
            c.databases[0].gsam_format = Some(format(kind));
            c.databases[0].segments[0].min_length = if kind == ImsGsamRecordFormat::U {
                12
            } else {
                2
            };
            c.databases[0].segments[0].max_length = if kind == ImsGsamRecordFormat::U {
                16
            } else {
                8
            };
            let mut hierarchy = catalog();
            hierarchy.databases[0].name = "HIERDB".into();
            let ImsPcbMetadata::Database(pcb) = &mut hierarchy.psbs[0].pcbs[0] else {
                panic!()
            };
            pcb.name = "HIERPCB".into();
            pcb.database = "HIERDB".into();
            c.databases.push(hierarchy.databases.remove(0));
            c.psbs[0].pcbs.push(hierarchy.psbs[0].pcbs.remove(0));
            service.install_metadata(c).unwrap();
            let batch = invocation_class(run, ServiceClass::Batch);
            service
                .execute(&batch, &request(run, ImsOperation::Schedule, 1, &[], b""))
                .unwrap();
            let mut hierarchical_insert = request(run, ImsOperation::Insert, 2, &["ROOT"], b"A1X");
            hierarchical_insert.pcb = 5;
            let feedback = HostRequest::ImsPcbFeedbackV1(ImsPcbFeedbackRequestV1 {
                request: hierarchical_insert,
                context: ImsExecutionContext::DbBatch,
                ssas: None,
                key_capacity: 64,
            });
            assert!(matches!(
                batch_public(service.clone(), run, feedback.clone()),
                HostResult::ImsPcbFeedbackV1(_)
            ));
            let insert_request = HostRequest::ImsGsam(insert(run, 3, kind, literals(kind).0));
            let inserted = batch_public(service.clone(), run, insert_request.clone());
            let epoch = {
                let durable = service.lock().unwrap();
                let state = &durable.state;
                assert!(state.generic_pending_undo[run].contains_key("GENDB"));
                assert!(state.generic_pending_undo[run].contains_key("HIERDB"));
                state.sessions[run].recovery.uow_epoch
            };
            assert_eq!(epoch, 0);
            drop(service);
            let fresh = ImsService::open(store, ImsLimits::default()).unwrap();
            assert_eq!(
                fresh.lock().unwrap().state.generic_pending_undo[run].len(),
                2
            );
            fresh
                .execute(&batch, &request(run, ImsOperation::Rollback, 4, &[], b""))
                .unwrap();
            assert!(
                !fresh
                    .lock()
                    .unwrap()
                    .state
                    .generic_pending_undo
                    .contains_key(run)
            );
            assert_eq!(
                fresh.lock().unwrap().state.sessions[run].recovery.uow_epoch,
                epoch + 1
            );
            assert_eq!(
                generic::restored(&fresh.lock().unwrap().state, "GENDB", ImsLimits::default())
                    .unwrap()
                    .record_count(),
                0
            );
            assert_eq!(
                generic::restored(&fresh.lock().unwrap().state, "HIERDB", ImsLimits::default())
                    .unwrap()
                    .record_count(),
                0
            );
            let before = snapshot(&fresh);
            assert_eq!(batch_public(fresh.clone(), run, insert_request), inserted);
            assert!(matches!(
                batch_public(fresh.clone(), run, feedback),
                HostResult::ImsPcbFeedbackV1(_)
            ));
            assert_eq!(snapshot(&fresh), before);
        });
    }
}

#[test]
fn gsam_formats_legacy_exact_replay_precedes_unowned_u_rejection() {
    backends("composition-legacy-replay", |store| {
        let run = "legacy-u-replay";
        let service = install(store, run, ImsGsamRecordFormat::U, ImsLimits::default());
        let legacy = request(run, ImsOperation::GetNext, 2, &[], b"");
        let result = status("GB");
        let recorded =
            RecordedResult::from_result(canonical_ims_request_digest(&legacy).unwrap(), &result);
        // A historical canonical receipt has no output extension or format operand.
        assert_eq!(
            serde_json::to_string(&crate::service::gsam::ReplayOutput {
                address: None,
                undefined_length: None
            })
            .unwrap(),
            "{\"address\":null}"
        );
        let mut durable = service.lock().unwrap();
        let mut next = durable.state.scoped_snapshot();
        next.replay.insert(format!("{run}-2"), Arc::new(recorded));
        service.persist(&mut durable, next).unwrap();
        drop(durable);
        let before = snapshot(&service);
        assert_eq!(service.execute(&invocation(run), &legacy).unwrap(), result);
        assert_eq!(snapshot(&service), before);
        assert_eq!(
            service.execute(
                &invocation(run),
                &request(run, ImsOperation::GetNext, 3, &[], b"")
            ),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(snapshot(&service), before);
    });
}

#[test]
fn gsam_formats_ambiguous_feedback_and_gsam_receipts_fail_closed() {
    let run = "format-ambiguity";
    let service = install(
        Arc::new(MemoryStore::new(Default::default())),
        run,
        ImsGsamRecordFormat::U,
        ImsLimits::default(),
    );
    let call = insert(
        run,
        2,
        ImsGsamRecordFormat::U,
        literals(ImsGsamRecordFormat::U).0,
    );
    let feedback = ImsPcbFeedbackRequestV1 {
        request: call.request.clone(),
        context: ImsExecutionContext::DbBatch,
        ssas: None,
        key_capacity: 64,
    };
    let before = snapshot(&service);
    assert!(matches!(
        service.execute_operands_at(
            &invocation(run),
            &call.request,
            100,
            None,
            Some(&call),
            Some(&feedback)
        ),
        Err(HostProblem::Malformed)
    ));
    assert_eq!(snapshot(&service), before);
    let output = public(service.clone(), run, call).unwrap();
    assert_eq!(output.undefined_length, None);
    let mut recorded = (*service.lock().unwrap().state.replay["format-ambiguity-2"]).clone();
    recorded.pcb_feedback_v1 = Some(mainframe_env_host_api::ImsPcbFeedbackV1 {
        pcb: 2,
        database: "GENDB".into(),
        processing_options: "L".into(),
        sensitive_segment_count: 1,
        transferred_data_length: 0,
        key: mainframe_env_host_api::ImsPcbKeyFeedbackV1::Unsupported(
            mainframe_env_host_api::ImsPcbFeedbackUnsupportedV1::NonKeyOperation,
        ),
    });
    assert!(
        validate_ims_recorded_result("format-ambiguity-2", &recorded, ImsLimits::default())
            .is_err()
    );
}
