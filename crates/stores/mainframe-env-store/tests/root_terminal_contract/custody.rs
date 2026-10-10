//! Physical store controls only; no compiled actor or MQ pending-work credit.
use super::*;

fn cutoff_preserves_and_fences(store: &dyn PlatformStore) {
    let (mut request, effect) = original(store);
    store.mutate_root_provider_states(request.clone()).unwrap();
    let before = observed(store, &request);
    let cutoff = store
        .capture_root_custody_cutoff(&request.occurrence.claim, &request.occurrence.execution, 11)
        .unwrap();
    assert_eq!(cutoff.preserved.actors.len(), 1);
    assert_eq!(
        cutoff.preserved.actors[0].execution,
        request.occurrence.execution
    );
    assert_eq!(cutoff.preserved.actors[0].effects, vec![effect]);
    assert_eq!(cutoff.preserved.closing.version + 1, cutoff.fenced.version);
    assert_eq!(cutoff.marker.namespace, ROOT_CUSTODY_NAMESPACE);
    assert_eq!(cutoff.marker.version, 1);
    let old: serde_json::Value = serde_json::from_slice(&cutoff.preserved.closing.payload).unwrap();
    let mut expected = old.clone();
    expected["phase"] = serde_json::json!(3);
    assert_eq!(old["phase"], 0);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&cutoff.fenced.payload).unwrap(),
        expected
    );
    let after = observed(store, &request);
    assert_eq!(
        (&after.0, &after.1, &after.2, &after.3, &after.4),
        (&before.0, &before.1, &before.2, &before.3, &before.4)
    );
    let retained = |rows: &[ProviderStateRecord]| {
        rows.iter()
            .filter(|row| {
                row.namespace != ROOT_CUSTODY_NAMESPACE
                    && !(row.namespace == ROOT_DRIVER_NAMESPACE
                        && row.key == request.occurrence.execution.execution_id.as_str())
            })
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(retained(&after.5), retained(&before.5));
    assert_eq!(after.7, 11);
    assert_eq!(after.6, cutoff.fenced_epoch);
    assert!(after.6 > before.6);
    request.occurrence.observed_tick = 11;
    request.mutations = vec![put("native-root-a", "first", 2, Some(1))];
    refuses(store, &request, request.clone());
    let frozen = observed(store, &request);
    assert!(
        store
            .capture_root_custody_cutoff(
                &request.occurrence.claim,
                &request.occurrence.execution,
                11
            )
            .is_err()
    );
    let notice = before.3.first().unwrap();
    assert!(
        store
            .mark_notification_delivered(&notice.notification_id, notice.version, 11)
            .is_err()
    );
    assert_eq!(observed(store, &request), frozen);
}
fn substituted_cutoff_refuses(store: &dyn PlatformStore) {
    let (request, _) = original(store);
    store.mutate_root_provider_states(request.clone()).unwrap();
    for fault in 0..6 {
        let mut actor = request.occurrence.execution.clone();
        let tick = match fault {
            0 => 0,
            1 => 9,
            2 => 100,
            _ => 11,
        };
        match fault {
            3 => actor.attempt += 1,
            4 => actor.version += 1,
            5 => actor.state = ExecutionState::Completed,
            _ => {}
        }
        let before = observed(store, &request);
        assert!(
            store
                .capture_root_custody_cutoff(&request.occurrence.claim, &actor, tick)
                .is_err(),
            "fault {fault}"
        );
        assert_eq!(observed(store, &request), before);
    }
}
#[test]
fn custody_memory_exact_preservation_and_stale_writer_controls() {
    cutoff_preserves_and_fences(&MemoryStore::new(StoreLimits::default()));
    substituted_cutoff_refuses(&MemoryStore::new(StoreLimits::default()));
}
#[test]
fn custody_sqlite_exact_preservation_and_stale_writer_controls() {
    let owned = OwnedSqlite::new();
    cutoff_preserves_and_fences(owned.store());
    let owned = OwnedSqlite::new();
    substituted_cutoff_refuses(owned.store());
}
