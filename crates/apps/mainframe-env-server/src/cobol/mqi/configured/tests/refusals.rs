use super::setup::*;
use super::*;
use mainframe_env_host_api::*;
use std::sync::atomic::Ordering;

#[test]
fn equal_clock_values_and_forwarded_provider_descriptor_do_not_prove_physical_identity() {
    for sqlite in [false, true] {
        for (wrapped, foreign_control) in [(true, false), (false, true)] {
            let f = Fixture::registered(sqlite, 16, wrapped, foreign_control);
            f.install("MQFLOW", SOURCE);
            let before = f.rows();
            let (_, reply) = f.run(f.effect(1, "MQFLOW"));
            assert_eq!(reply.outcome, Err(HostProblem::Unauthorized));
            assert_eq!(f.rows(), before);
            assert!(f.mq.topology.lock().unwrap().roots.is_empty());
            assert!(f.saf.resources.lock().unwrap().is_empty());
        }
    }
}

#[test]
fn real_call_denied_saf_and_late_cancel_never_publish_selected_rows() {
    for sqlite in [false, true] {
        for cancel in [false, true] {
            let f = Fixture::new(sqlite);
            f.install("MQFLOW", SOURCE);
            let before = f.rows();
            if cancel {
                let probe = f.parent.cancellation_probe.clone().unwrap();
                *f.saf.hook.lock().unwrap() = Some(Box::new(move || probe.request()));
            } else {
                f.saf.deny.store(true, Ordering::SeqCst);
            }
            let (outcome, reply) = f.run_raw(f.effect(1, "MQFLOW"));
            assert!(
                reply.as_ref().is_none_or(|r| r.outcome.is_err()),
                "{outcome:?} {reply:?}"
            );
            assert_eq!(f.rows(), before);
            assert!(
                f.store
                    .list_provider_state("mq-selected-v1-occurrence", 10)
                    .unwrap()
                    .is_empty()
            );
            assert!(!f.saf.resources.lock().unwrap().is_empty());
            let child = f.factory.children.lock().unwrap()[0].clone();
            assert!(
                f.factory.observations.lock().unwrap()[0]
                    .profile(&child)
                    .is_err()
            );
        }
    }
}

#[test]
fn genuine_proof_cannot_select_foreign_physical_host_even_with_equal_rows() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let foreign = Fixture::new(sqlite);
        f.install("MQFLOW", SOURCE);
        assert_eq!(f.rows(), foreign.rows());
        *f.factory.override_host.lock().unwrap() = Some(foreign.mq.clone());
        let before = f.rows();
        let (_, reply) = f.run(f.effect(1, "MQFLOW"));
        assert_eq!(reply.outcome, Err(HostProblem::Unauthorized));
        assert_eq!(f.rows(), before);
        assert_eq!(foreign.rows(), before);
        assert!(f.saf.resources.lock().unwrap().is_empty());
        assert!(foreign.saf.resources.lock().unwrap().is_empty());
        assert!(f.mq.topology.lock().unwrap().roots.is_empty());
    }
}

#[test]
fn actual_preparation_abort_revokes_observation_preserves_retained_root() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        f.install("MQFLOW", SOURCE);
        let before = f.rows();
        f.factory.fail_preparation.store(true, Ordering::SeqCst);
        let (_, reply) = f.run(f.effect(1, "MQFLOW"));
        assert!(reply.outcome.is_err());
        assert_eq!(f.rows(), before);
        let child = f.factory.children.lock().unwrap()[0].clone();
        assert!(
            f.factory.observations.lock().unwrap()[0]
                .profile(&child)
                .is_err()
        );
        let parent = f.mq.parent_frame(&f.parent).unwrap();
        parent.check_original(&f.parent).unwrap();
        assert!(f.saf.resources.lock().unwrap().is_empty());
    }
}
