use super::trace_tests::{backends, cursor, rows};
use super::*;

#[test]
fn ssa_primary_consumer_feedback_conditions_never_reuse_persistent_prefix() {
    for condition in [0, 1] {
        backends("feedback-condition", |store, _| {
            let run = "primary-feedback-condition";
            let mut catalog = metadata();
            let ImsPcbMetadata::Database(pcb) = &mut catalog.psbs[0].pcbs[0] else {
                panic!()
            };
            if condition == 0 {
                pcb.sensitive_segments.retain(|s| s.name != "C");
            } else {
                pcb.sensitive_segments
                    .iter_mut()
                    .find(|s| s.name == "C")
                    .unwrap()
                    .processing_options = Some("I".into());
            }
            let service = seed_catalog(store.clone(), run, catalog);
            nav(&service, run, 5, ImsOperation::GetHoldUnique, PREFIX).unwrap();
            let before = cursor(&service, run, 1);
            let HostResult::ImsPcbFeedbackV1(result) = invoke(
                &service,
                run,
                HostRequest::ImsPcbFeedbackV1(ImsPcbFeedbackRequestV1 {
                    request: request(run, ImsOperation::GetHoldNext, 6, &[], b""),
                    context: ImsExecutionContext::DbBatch,
                    ssas: Some(UU.iter().map(|s| s.to_vec()).collect()),
                    key_capacity: 5,
                }),
            )
            .unwrap() else {
                panic!()
            };
            assert_eq!(
                result.result.status,
                if condition == 0 { "AC" } else { "AM" }
            );
            assert_eq!(
                result.feedback.key,
                ImsPcbKeyFeedbackV1::Unsupported(
                    mainframe_env_host_api::ImsPcbFeedbackUnsupportedV1::FailedCallWitness
                )
            );
            assert_eq!(result.feedback.transferred_data_length, 0);
            assert_eq!(cursor(&service, run, 1), before);
            eprintln!("PRIMARY-FEEDBACK-CONDITION-PASS {}", result.result.status);
        });
    }
}

pub(super) fn legacy(service: &Arc<ImsService>, run: &str, req: ImsRequest) -> ImsResult {
    let HostResult::Ims(result) = invoke(service, run, HostRequest::Ims(req)).unwrap() else {
        panic!()
    };
    result
}

#[test]
fn ssa_primary_consumer_parent_and_root_delete_cancel_removed_authority() {
    for root in [false, true] {
        backends("delete-ancestor", |store, _| {
            let run = "primary-delete-ancestor";
            let service = seed(store, run);
            let ssas = if root { &PREFIX[..1] } else { PREFIX };
            let result = nav(&service, run, 5, ImsOperation::GetHoldUnique, ssas).unwrap();
            assert_eq!(
                result.segments[0].data,
                if root { b"A1r".as_slice() } else { b"B1114x" }
            );
            let result = legacy(
                &service,
                run,
                request(run, ImsOperation::Delete, 6, &[], b""),
            );
            assert_eq!(result.status, "  ");
            assert_eq!(result.affected_segments, if root { 9 } else { 4 });
            let trace = cursor(&service, run, 1);
            assert!(trace["held"].is_null());
            assert!(trace["parentage"].is_null());
            assert_eq!(trace["primary_search"]["boundary"]["provenance"], "deleted");
            assert!(trace["primary_search"]["levels"][1].is_null());
            assert!(trace["primary_search"]["levels"][2].is_null());
            if root {
                assert!(trace["primary_search"]["levels"][0].is_null());
            }
            assert_eq!(
                nav(&service, run, 7, ImsOperation::GetNext, UU),
                Err(HostProblem::Unsupported)
            );
            assert_eq!(
                nav(&service, run, 8, ImsOperation::GetNext, &[])
                    .unwrap()
                    .segments[0]
                    .data,
                if root { b"A2r".as_slice() } else { b"B1215x" }
            );
        });
    }
}

#[test]
fn ssa_primary_consumer_insert_parentage_duplicate_and_backout_load_cancellation() {
    backends("insert-parentage", |store, _| {
        let run = "primary-insert-parentage";
        let service = seed(store.clone(), run);
        nav(&service, run, 5, ImsOperation::GetHoldUnique, &PREFIX[..1]).unwrap();
        let mut insert = request(run, ImsOperation::Insert, 6, &["C"], b"C113n");
        insert.qualifiers = vec![
            ImsQualifier {
                segment: "A".into(),
                field: "AKEY".into(),
                value: b"A1".to_vec(),
            },
            ImsQualifier {
                segment: "B".into(),
                field: "BKEY".into(),
                value: b"B11".to_vec(),
            },
        ];
        assert_eq!(legacy(&service, run, insert).status, "  ");
        let trace = cursor(&service, run, 1);
        assert_eq!(trace["parentage"], 1);
        assert_eq!(trace["current"], 13);
        assert!(trace["held"].is_null());
        let before = cursor(&service, run, 1);
        assert_eq!(
            legacy(
                &service,
                run,
                request(run, ImsOperation::Insert, 7, &["C"], b"C113q")
            )
            .status,
            "II"
        );
        assert_eq!(cursor(&service, run, 1), before);
        assert_eq!(
            legacy(
                &service,
                run,
                request(run, ImsOperation::Rollback, 8, &[], b"")
            )
            .status,
            "  "
        );
        assert_eq!(
            cursor(&service, run, 1),
            serde_json::json!({"current":null,"parentage":null,"held":null,"after_end":false})
        );
        assert_eq!(
            nav(&service, run, 9, ImsOperation::GetNext, UU),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(
            nav(&service, run, 10, ImsOperation::GetUnique, EXACT_MISS)
                .unwrap()
                .status,
            "GE"
        );
        nav(&service, run, 11, ImsOperation::GetHoldUnique, PREFIX).unwrap();
        let mut load = request(run, ImsOperation::Load, 12, &[], b"");
        load.data = serde_json::to_vec(&image()).unwrap();
        assert_eq!(legacy(&service, run, load).status, "  ");
        assert!(cursor(&service, run, 1).get("primary_search").is_none());
        assert_eq!(
            nav(&service, run, 13, ImsOperation::GetNext, UU),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(
            nav(&service, run, 14, ImsOperation::GetUnique, PREFIX)
                .unwrap()
                .segments[0]
                .data,
            b"B1114x"
        );
    });
}

#[test]
fn ssa_primary_consumer_other_pcb_deleted_boundary_and_equal_key_no_rebinding() {
    backends("pcb-delete-reinsert", |store, _| {
        let run = "primary-pcb-delete-reinsert";
        let mut catalog = metadata();
        let mut second = catalog.psbs[0].pcbs[0].clone();
        let ImsPcbMetadata::Database(pcb) = &mut second else {
            panic!()
        };
        pcb.name = "SECOND".into();
        catalog.psbs[0].pcbs.push(second);
        let service = seed_catalog(store.clone(), run, catalog);
        nav(&service, run, 5, ImsOperation::GetNext, DATA_MISS).unwrap();
        let mut get = navigation(
            run,
            6,
            ImsOperation::GetHoldUnique,
            &[b"A       (AKEY    EQA1)", b"B       (BKEY    EQB13)"],
        );
        get.request.pcb = 2;
        let HostResult::Ims(result) =
            invoke(&service, run, HostRequest::ImsNavigation(get)).unwrap()
        else {
            panic!()
        };
        assert_eq!(result.segments[0].data, b"B1316x");
        let mut delete = request(run, ImsOperation::Delete, 7, &[], b"");
        delete.pcb = 2;
        assert_eq!(legacy(&service, run, delete).status, "  ");
        let trace = cursor(&service, run, 1);
        assert_eq!(trace["primary_search"]["boundary"]["provenance"], "deleted");
        assert_eq!(trace["primary_search"]["boundary"]["anchor_id"], 8);
        assert_eq!(
            trace["primary_search"]["levels"][1]["key"],
            serde_json::json!([66, 49, 49])
        );
        // Insert a new occurrence at the deleted key through another PCB. It
        // must not turn the first PCB's old order address into live authority.
        let mut insert = request(run, ImsOperation::Insert, 8, &["B"], b"B1319n");
        insert.pcb = 2;
        insert.qualifiers = vec![ImsQualifier {
            segment: "A".into(),
            field: "AKEY".into(),
            value: b"A1".to_vec(),
        }];
        assert_eq!(legacy(&service, run, insert).status, "  ");
        assert!(cursor(&service, run, 1).get("primary_search").is_none());
        assert_eq!(cursor(&service, run, 2)["current"], 13);
        let before = rows(&*store);
        assert_eq!(
            nav(&service, run, 9, ImsOperation::GetNext, UU),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(rows(&*store), before);
        assert_eq!(
            nav(&service, run, 10, ImsOperation::GetUnique, PREFIX)
                .unwrap()
                .segments[0]
                .data,
            b"B1114x"
        );
        assert_eq!(
            nav(&service, run, 11, ImsOperation::GetNext, UU)
                .unwrap()
                .segments[0]
                .data,
            b"C111a"
        );
    });
}

#[test]
fn ssa_primary_consumer_insert_uses_satisfied_b11_not_examined_b13() {
    backends("insert-satisfied", |store, _| {
        let run = "primary-insert-satisfied";
        let service = seed(store, run);
        assert_eq!(
            nav(&service, run, 5, ImsOperation::GetNext, DATA_MISS)
                .unwrap()
                .status,
            "GE"
        );
        let result = legacy(
            &service,
            run,
            request(run, ImsOperation::Insert, 6, &["C"], b"C113n"),
        );
        assert_eq!(result.status, "  ");
        assert_eq!(result.affected_segments, 1);
        let trace = cursor(&service, run, 1);
        assert_eq!(
            trace["primary_search"]["levels"][1]["key"],
            serde_json::json!([66, 49, 49])
        );
        assert_eq!(
            trace["primary_search"]["levels"][2]["key"],
            serde_json::json!([67, 49, 49, 51])
        );
        assert_eq!(trace["primary_search"]["boundary"]["anchor_id"], 13);
        assert_eq!(trace["parentage"], serde_json::Value::Null);
        assert_eq!(trace["held"], serde_json::Value::Null);
        assert_eq!(
            nav(&service, run, 7, ImsOperation::GetNext, &[])
                .unwrap()
                .segments[0]
                .data,
            b"D111e"
        );
    });
}

#[test]
fn ssa_primary_consumer_delete_leaf_keeps_gap_and_surviving_levels() {
    backends("delete-gap", |store, _| {
        let run = "primary-delete-gap";
        let service = seed(store, run);
        assert_eq!(
            nav(
                &service,
                run,
                5,
                ImsOperation::GetHoldUnique,
                &[
                    b"A       (AKEY    EQA1)",
                    b"B       (BKEY    EQB11)",
                    b"C       (CKEY    EQC112)"
                ]
            )
            .unwrap()
            .segments[0]
                .data,
            b"C112b"
        );
        let result = legacy(
            &service,
            run,
            request(run, ImsOperation::Delete, 6, &[], b""),
        );
        assert_eq!(result.status, "  ");
        assert_eq!(result.affected_segments, 1);
        let trace = cursor(&service, run, 1);
        assert_eq!(trace["primary_search"]["boundary"]["provenance"], "deleted");
        assert_eq!(trace["primary_search"]["boundary"]["anchor_id"], 4);
        assert_eq!(
            trace["primary_search"]["levels"][1]["key"],
            serde_json::json!([66, 49, 49])
        );
        assert_eq!(
            trace["primary_search"]["levels"][2],
            serde_json::Value::Null
        );
        assert_eq!(
            trace["primary_search"]["feedback_path"][2]["key"],
            serde_json::json!([67, 49, 49, 50])
        );
        assert_eq!(trace["held"], serde_json::Value::Null);
        assert_eq!(
            nav(&service, run, 7, ImsOperation::GetNext, UU)
                .unwrap()
                .status,
            "GE"
        );
        assert_eq!(
            nav(&service, run, 8, ImsOperation::GetNext, UU)
                .unwrap()
                .status,
            "GE"
        );
        assert_eq!(
            nav(&service, run, 9, ImsOperation::GetNext, &[])
                .unwrap()
                .segments[0]
                .data,
            b"D111e"
        );
    });
}

#[test]
fn ssa_primary_consumer_replace_refreshes_real_observed_version() {
    backends("replace-version", |store, _| {
        let run = "primary-replace-version";
        let service = seed(store, run);
        assert_eq!(
            nav(&service, run, 5, ImsOperation::GetHoldNext, UU)
                .unwrap()
                .segments[0]
                .data,
            b"C111a"
        );
        let original = cursor(&service, run, 1);
        assert_eq!(
            legacy(
                &service,
                run,
                request(run, ImsOperation::Replace, 6, &[], b"C111z")
            )
            .status,
            "  "
        );
        let changed = cursor(&service, run, 1);
        assert_eq!(
            changed["primary_search"]["boundary"],
            original["primary_search"]["boundary"]
        );
        assert_eq!(
            changed["primary_search"]["levels"][2]["observed_version"],
            2
        );
        assert_eq!(changed["held"]["version"], 2);
        assert_eq!(
            nav(&service, run, 7, ImsOperation::GetHoldNext, UU)
                .unwrap()
                .segments[0]
                .data,
            b"C112b"
        );
    });
}

#[test]
fn ssa_primary_consumer_failed_insert_parent_publishes_real_gu_prefix() {
    backends("insert-parent-failure", |store, _| {
        let run = "primary-insert-parent-failure";
        let service = seed(store, run);
        nav(&service, run, 5, ImsOperation::GetHoldNext, UU).unwrap();
        let mut insert = request(run, ImsOperation::Insert, 6, &["C"], b"C141n");
        insert.qualifiers = vec![
            ImsQualifier {
                segment: "A".into(),
                field: "AKEY".into(),
                value: b"A1".to_vec(),
            },
            ImsQualifier {
                segment: "B".into(),
                field: "BKEY".into(),
                value: b"B14".to_vec(),
            },
        ];
        let result = legacy(&service, run, insert);
        assert_eq!(result.status, "GE");
        let trace = cursor(&service, run, 1);
        assert_eq!(
            trace["primary_search"]["feedback_path"],
            serde_json::json!([{"segment":"A","key":[65,49]}])
        );
        assert_eq!(trace["primary_search"]["boundary"]["kind"], "missing");
        assert_eq!(trace["held"], serde_json::Value::Null);
        assert_eq!(
            nav(&service, run, 7, ImsOperation::GetNext, &[])
                .unwrap()
                .segments[0]
                .data,
            b"E11f"
        );
    });
}

#[test]
fn ssa_primary_consumer_legacy_batch_gu_builds_actual_certificate() {
    backends("legacy-gu", |store, _| {
        let run = "primary-legacy-gu";
        let service = seed(store, run);
        let mut get = request(run, ImsOperation::GetHoldUnique, 5, &["B"], b"");
        get.qualifiers = vec![
            ImsQualifier {
                segment: "A".into(),
                field: "AKEY".into(),
                value: b"A1".to_vec(),
            },
            ImsQualifier {
                segment: "B".into(),
                field: "BKEY".into(),
                value: b"B12".to_vec(),
            },
        ];
        assert_eq!(legacy(&service, run, get).segments[0].data, b"B1215x");
        assert_eq!(
            cursor(&service, run, 1)["primary_search"]["levels"][1]["key"],
            serde_json::json!([66, 49, 50])
        );
        assert_eq!(
            nav(&service, run, 6, ImsOperation::GetHoldNext, V)
                .unwrap()
                .segments[0]
                .data,
            b"C121c"
        );
    });
}

#[test]
fn ssa_primary_consumer_fresh_failed_feedback_capacity_has_no_publication() {
    backends("failed-feedback-capacity", |store, _| {
        let run = "primary-feedback-capacity";
        let service = seed(store.clone(), run);
        nav(&service, run, 5, ImsOperation::GetHoldUnique, PREFIX).unwrap();
        let before = rows(&*store);
        let req = HostRequest::ImsPcbFeedbackV1(ImsPcbFeedbackRequestV1 {
            request: request(run, ImsOperation::GetNext, 6, &[], b""),
            context: ImsExecutionContext::DbBatch,
            ssas: Some(DATA_MISS.iter().map(|s| s.to_vec()).collect()),
            key_capacity: 4,
        });
        assert_eq!(
            invoke(&service, run, req),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(rows(&*store), before);
    });
}
