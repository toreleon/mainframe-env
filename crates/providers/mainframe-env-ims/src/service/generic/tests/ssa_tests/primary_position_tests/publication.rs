//! Covered mutations use the existing atomic proposal and canonical receipt.
use super::super::super::session_cas::SessionCasStore;
use super::fences::legacy;
use super::trace_tests::{backends, cursor, rows};
use super::*;

#[test]
fn ssa_primary_consumer_insert_capacity_cas_lost_ack_and_exact_replay() {
    backends("insert-publication", |store, _| {
        let run = "primary-insert-publication";
        let service = seed(store.clone(), run);
        assert_eq!(
            nav(&service, run, 5, ImsOperation::GetNext, DATA_MISS)
                .unwrap()
                .status,
            "GE"
        );
        let before = rows(&*store);
        let count = service.lock().unwrap().state.replay.len();
        drop(service);
        let limited = ImsService::open(
            store.clone(),
            ImsLimits {
                max_replays: count,
                ..Default::default()
            },
        )
        .unwrap();
        let insert = HostRequest::Ims(request(run, ImsOperation::Insert, 6, &["C"], b"C113n"));
        assert_eq!(
            invoke(&limited, run, insert.clone()),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(rows(&*store), before);
        drop(limited);
        let raced = SessionCasStore::new(store.clone(), run);
        let service = ImsService::open(raced.clone(), Default::default()).unwrap();
        raced.arm();
        assert_eq!(
            invoke(&service, run, insert.clone()),
            Err(HostProblem::IdempotencyConflict)
        );
        let after = rows(&*store);
        assert_eq!(after.len(), before.len());
        for (a, b) in before.iter().zip(&after) {
            assert_eq!(a.payload, b.payload);
            if a.namespace == SESSION_NAMESPACE && a.key == run {
                assert_eq!(b.version, a.version + 1);
            } else {
                assert_eq!(a, b);
            }
        }
        drop(service);
        let service = ImsService::open(raced.clone(), Default::default()).unwrap();
        raced.lose_ack();
        assert_eq!(
            invoke(&service, run, insert.clone()),
            Err(HostProblem::UnknownOutcome)
        );
        drop(service);
        let service = ImsService::open(store.clone(), Default::default()).unwrap();
        assert_eq!(cursor(&service, run, 1)["current"], 13);
        assert_eq!(
            cursor(&service, run, 1)["primary_search"]["levels"][1]["key"],
            serde_json::json!([66, 49, 49])
        );
        assert!(cursor(&service, run, 1)["held"].is_null());
        let published = rows(&*store);
        let HostResult::Ims(result) = invoke(&service, run, insert.clone()).unwrap() else {
            panic!()
        };
        assert_eq!(result.affected_segments, 1);
        assert_eq!(rows(&*store), published);
        assert_eq!(
            nav(&service, run, 7, ImsOperation::GetNext, &[])
                .unwrap()
                .segments[0]
                .data,
            b"D111e"
        );
        let moved = rows(&*store);
        invoke(&service, run, insert).unwrap();
        assert_eq!(rows(&*store), moved);
        eprintln!("PRIMARY-INSERT-PUBLICATION-PASS capacity/CAS/lost-ack/exact-replay");
    });
}

#[test]
fn ssa_primary_consumer_delete_lost_ack_replays_removed_count_and_keeps_gap() {
    backends("delete-publication", |store, _| {
        let run = "primary-delete-publication";
        let service = seed(store.clone(), run);
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
        drop(service);
        let raced = SessionCasStore::new(store.clone(), run);
        let service = ImsService::open(raced.clone(), Default::default()).unwrap();
        raced.lose_ack();
        let delete = HostRequest::Ims(request(run, ImsOperation::Delete, 6, &[], b""));
        assert_eq!(
            invoke(&service, run, delete.clone()),
            Err(HostProblem::UnknownOutcome)
        );
        drop(service);
        let service = ImsService::open(store.clone(), Default::default()).unwrap();
        assert_eq!(
            cursor(&service, run, 1)["primary_search"]["boundary"]["provenance"],
            "deleted"
        );
        assert!(cursor(&service, run, 1)["primary_search"]["levels"][2].is_null());
        assert_eq!(
            nav(&service, run, 7, ImsOperation::GetNext, &[])
                .unwrap()
                .segments[0]
                .data,
            b"D111e"
        );
        let moved = rows(&*store);
        let HostResult::Ims(result) = invoke(&service, run, delete).unwrap() else {
            panic!()
        };
        assert_eq!(result.affected_segments, 1);
        assert_eq!(rows(&*store), moved);
        assert_eq!(
            legacy(
                &service,
                run,
                request(run, ImsOperation::Replace, 8, &[], b"D111q")
            )
            .status,
            "DJ"
        );
        eprintln!("PRIMARY-DELETE-PUBLICATION-PASS lost-ack/deleted-gap/exact-replay/no-hold");
    });
}
