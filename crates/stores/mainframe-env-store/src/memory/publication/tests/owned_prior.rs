use super::*;

fn prior_state(store: &MemoryStore) -> (ProviderStateRecord, usize, usize, u64) {
    let state = store.lock().unwrap();
    let row = state
        .provider_state
        .get(&("publication-fixture-v1".into(), "queue".into()))
        .unwrap();
    (
        row.clone(),
        row.payload.as_ptr() as usize,
        state.blob_bytes,
        state.provider_epoch,
    )
}

fn assert_prior_restored(store: &MemoryStore, before: &(ProviderStateRecord, usize, usize, u64)) {
    let state = store.lock().unwrap();
    let row = state
        .provider_state
        .get(&("publication-fixture-v1".into(), "queue".into()))
        .unwrap();
    assert_eq!(row, &before.0);
    assert_eq!(row.payload.as_ptr() as usize, before.1);
    assert_eq!(state.blob_bytes, before.2);
    assert_eq!(state.provider_epoch, before.3);
    assert_eq!(state.provider_state.len(), 1);
}

#[test]
fn repeated_puts_restore_the_original_row_after_late_batch_failure() {
    for fault in 0..3 {
        let mut store = MemoryStore::new(StoreLimits::default());
        store
            .mutate_provider_states_atomic(vec![put("queue", 1, None, &[7; 64])])
            .unwrap();
        let normal_epoch = store.lock().unwrap().provider_epoch;
        if fault == 2 {
            store.lock().unwrap().provider_epoch = u64::MAX - 1;
        }
        let before = prior_state(&store);
        let mut writes = vec![
            put("queue", 2, Some(1), &[8; 96]),
            put("queue", 3, Some(2), &[9; 128]),
        ];
        let expected = match fault {
            0 => {
                writes.push(put("queue", 5, Some(3), b"wrong successor"));
                StoreError::Conflict
            }
            1 => {
                store.limits.max_total_blob_bytes = 127;
                StoreError::CapacityExceeded
            }
            2 => StoreError::CapacityExceeded,
            _ => unreachable!(),
        };
        assert_eq!(store.mutate_provider_states_atomic(writes), Err(expected));
        assert_prior_restored(&store, &before);
        store.limits = StoreLimits::default();
        store.lock().unwrap().provider_epoch = normal_epoch;
        store
            .mutate_provider_states_atomic(vec![put("queue", 2, Some(1), b"retry")])
            .unwrap();
        let row = store
            .get_provider_state("publication-fixture-v1", "queue")
            .unwrap()
            .unwrap();
        assert_eq!((row.version, row.payload), (2, b"retry".to_vec()));
    }
}

#[test]
fn audited_late_failure_restores_rows_and_audit_metadata() {
    let mut store = MemoryStore::new(StoreLimits::default());
    let mut request = prepare(&store);
    store
        .mutate_provider_states_atomic(vec![put("queue", 1, None, &[7; 64])])
        .unwrap();
    let before = prior_state(&store);
    let metadata = {
        let state = store.lock().unwrap();
        (state.logical_tick, state.next_audit_ordinal)
    };
    request.mutations = vec![
        put("queue", 2, Some(1), &[8; 96]),
        put("queue", 3, Some(2), &[9; 128]),
    ];
    store.limits.max_audits = 0;
    assert_eq!(
        store.publish_provider_states_audited(request.clone()),
        Err(StoreError::CapacityExceeded)
    );
    assert_prior_restored(&store, &before);
    let state = store.lock().unwrap();
    assert_eq!((state.logical_tick, state.next_audit_ordinal), metadata);
    assert!(state.audits.is_empty());
    assert_eq!(
        state.effects.get(&request.intent.key),
        Some(&request.intent)
    );
}

#[test]
fn successful_put_keeps_owned_new_payload_and_stale_cas_cannot_replace_it() {
    let store = MemoryStore::new(StoreLimits::default());
    store
        .mutate_provider_states_atomic(vec![put("queue", 1, None, &[7; 64])])
        .unwrap();
    let mut next = put("queue", 2, Some(1), &[8; 96]);
    let ProviderStateMutation::Put(write) = &mut next else {
        unreachable!()
    };
    let address = write.record.payload.as_ptr() as usize;
    store.mutate_provider_states_atomic(vec![next]).unwrap();
    let committed = prior_state(&store);
    assert_eq!(committed.0.version, 2);
    assert_eq!(committed.0.payload, [8; 96]);
    assert_eq!(committed.1, address);
    assert_eq!(
        store.mutate_provider_states_atomic(vec![put("queue", 2, Some(1), b"stale")]),
        Err(StoreError::Conflict)
    );
    assert_prior_restored(&store, &committed);
}
