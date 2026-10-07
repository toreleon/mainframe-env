use super::*;
use crate::replay_refusal::tests::{request, snapshot};
use mainframe_env_store_api::JournalStore;

#[test]
fn checked_replay_refusal_memory_late_quota_ordinal_epoch_and_outbox_rollback() {
    for fault in 0..5 {
        let mut store = MemoryStore::new(StoreLimits::default());
        let r = request(&store);
        match fault {
            0 => store.limits.max_audits = 0,
            1 => store.limits.max_outbox = 0,
            2 => store.limits.max_events = 3,
            3 => store.lock().unwrap().next_audit_ordinal = u64::MAX,
            4 => store.lock().unwrap().provider_epoch = u64::MAX - 1,
            _ => unreachable!(),
        }
        let fault_limits = store.limits;
        store.limits = StoreLimits::default();
        let before = snapshot(&store, &r);
        let counters = {
            let s = store.lock().unwrap();
            (s.blob_bytes, s.logical_tick, s.next_audit_ordinal)
        };
        store.limits = fault_limits;
        assert!(store.commit_checked_replay_refusal(r.clone()).is_err());
        store.limits = StoreLimits::default();
        assert_eq!(snapshot(&store, &r), before, "fault {fault}");
        let s = store.lock().unwrap();
        assert_eq!(
            (s.blob_bytes, s.logical_tick, s.next_audit_ordinal),
            counters
        );
    }
}
#[test]
fn checked_replay_refusal_memory_current_clock_floor_and_execution_expiry_are_live() {
    for expired in [false, true] {
        let store = MemoryStore::new(StoreLimits::default());
        let mut r = request(&store);
        {
            let mut s = store.lock().unwrap();
            if expired {
                r.execution.lease_expiry_tick = Some(10);
                s.executions
                    .insert(r.execution.execution_id.clone(), r.execution.clone());
            } else {
                s.logical_tick = 11;
            }
        }
        let before = snapshot(&store, &r);
        assert!(store.commit_checked_replay_refusal(r.clone()).is_err());
        assert_eq!(snapshot(&store, &r), before);
    }
}
