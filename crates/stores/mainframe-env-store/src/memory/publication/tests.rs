use super::*;
use crate::publication::tests::{prepare, put};

#[test]
fn audited_publication_faults_restore_touched_rows_bytes_epochs_clock_and_ordinals_without_cloning()
{
    for fault in 0..6 {
        let mut store = MemoryStore::new(StoreLimits::default());
        let mut request = prepare(&store);
        request.mutations = vec![put("queue", 1, None, b"x")];
        let normal_epoch = store.lock().unwrap().provider_epoch;
        let expected = match fault {
            0 => {
                store.limits.max_audits = 0;
                StoreError::CapacityExceeded
            }
            1 => {
                store.limits.max_blob_bytes = 1;
                StoreError::PayloadTooLarge
            }
            2 => {
                store.limits.max_provider_state = 0;
                StoreError::CapacityExceeded
            }
            3 => {
                store.limits.max_total_blob_bytes = 0;
                StoreError::CapacityExceeded
            }
            // The put succeeds, then the audit insert succeeds, then the epoch
            // bump fails. This kills a mutant that journals the key too late.
            4 => {
                store.lock().unwrap().provider_epoch = u64::MAX - 1;
                StoreError::CapacityExceeded
            }
            5 => {
                store.lock().unwrap().next_audit_ordinal = u64::MAX;
                StoreError::CapacityExceeded
            }
            _ => unreachable!(),
        };
        let before = {
            let state = store.lock().unwrap();
            (
                state.blob_bytes,
                state.provider_epoch,
                state.logical_tick,
                state.next_audit_ordinal,
            )
        };
        let clones = store.clone_count();
        assert_eq!(
            store.publish_provider_states_audited(request.clone()),
            Err(expected),
            "fault {fault}"
        );
        let state = store.lock().unwrap();
        assert_eq!(
            (
                state.blob_bytes,
                state.provider_epoch,
                state.logical_tick,
                state.next_audit_ordinal
            ),
            before
        );
        assert!(state.provider_state.is_empty());
        assert!(state.audits.is_empty());
        assert_eq!(
            state.effects.get(&request.intent.key),
            Some(&request.intent)
        );
        drop(state);
        assert_eq!(store.clone_count(), clones);
        store.limits = StoreLimits::default();
        {
            let mut state = store.lock().unwrap();
            state.provider_epoch = normal_epoch;
            state.next_audit_ordinal = 0;
        }
        request.mutations.clear();
        request.observed_tick = 8;
        request.audit.observed_tick = 8;
        store
            .publish_provider_states_audited(request.clone())
            .unwrap();
        let state = store.lock().unwrap();
        assert_eq!(state.logical_tick, 8);
        assert_eq!(state.next_audit_ordinal, 1);
        assert_eq!(state.audits.values().next(), Some(&request.audit));
        assert!(
            state
                .audits
                .keys()
                .next()
                .unwrap()
                .ends_with("memory:00000000000000000001")
        );
        assert_eq!(store.clone_count(), clones);
    }
}

#[test]
fn audited_publication_legacy_clock_and_execution_attempt_or_lease_are_fenced() {
    for fault in 0..4 {
        let store = MemoryStore::new(StoreLimits::default());
        let request = prepare(&store);
        match fault {
            0 => {
                store
                    .put_provider_state(
                        ProviderStateRecord {
                            namespace: "jes-worker-meta".into(),
                            key: "logical-clock".into(),
                            version: 1,
                            payload: 10_u64.to_be_bytes().to_vec(),
                        },
                        None,
                    )
                    .unwrap();
            }
            1 => {
                store
                    .lock()
                    .unwrap()
                    .executions
                    .get_mut(&request.intent.execution_id)
                    .unwrap()
                    .attempt = 2;
            }
            2 => {
                store
                    .lock()
                    .unwrap()
                    .executions
                    .get_mut(&request.intent.execution_id)
                    .unwrap()
                    .lease_expiry_tick = Some(9);
            }
            3 => {
                store
                    .lock()
                    .unwrap()
                    .executions
                    .get_mut(&request.intent.execution_id)
                    .unwrap()
                    .run_unit_id = mainframe_env_execution_api::RunUnitId::new(
                    "new-run",
                    mainframe_env_execution_api::InvocationLimits::default(),
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let epoch = store.provider_state_retention_epoch().unwrap();
        assert!(
            store
                .publish_provider_states_audited(request.clone())
                .is_err()
        );
        assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
        assert_eq!(
            store
                .get_provider_state("publication-fixture-v1", "queue")
                .unwrap(),
            None
        );
        assert!(store.lock().unwrap().audits.is_empty());
    }
}
