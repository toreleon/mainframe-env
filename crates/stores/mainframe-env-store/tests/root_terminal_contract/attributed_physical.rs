//! Owned physical corruption/fault fixtures, not valid producer authorities.
use super::*;
use sqlx::Connection;

#[test]
fn sqlite_corrupt_scope_or_actor_indexes_refuse_original_and_audit() {
    for fault in 0..4 {
        let fixture = OwnedSqlite::new();
        let store = fixture.store();
        let (request, effect) = original(store);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let mut connection = sqlx::SqliteConnection::connect(&format!(
                "sqlite://{}?mode=rw",
                fixture.path.display()
            ))
            .await
            .unwrap();
            let (namespace, key) = if fault == 3 {
                ("durable-root-actor-v1", "writer-a")
            } else {
                ("durable-root-scope-v1", "native-root-a")
            };
            if fault == 0 || fault == 3 {
                sqlx::query("DELETE FROM provider_state WHERE namespace=? AND key=?")
                    .bind(namespace)
                    .bind(key)
                    .execute(&mut connection)
                    .await
                    .unwrap();
            } else {
                sqlx::query(
                    "UPDATE provider_state SET version=?,payload=? WHERE namespace=? AND key=?",
                )
                .bind(if fault == 1 { 2 } else { 1 })
                .bind(if fault == 2 {
                    b"foreign-root".as_slice()
                } else {
                    b"writer-a".as_slice()
                })
                .bind(namespace)
                .bind(key)
                .execute(&mut connection)
                .await
                .unwrap();
            }
            connection.close().await.unwrap();
        });
        refuses(store, &request, request.clone());
        let before = observed(store, &request);
        assert!(
            store
                .publish_provider_states_audited(audit(
                    &request,
                    &effect,
                    request.mutations.clone()
                ))
                .is_err(),
            "fault {fault}"
        );
        assert_eq!(observed(store, &request), before);
    }
}

#[test]
fn sqlite_epoch_exhaustion_restores_whole_physical_state() {
    let fixture = OwnedSqlite::new();
    let store = fixture.store();
    let (mut request, _) = original(store);
    request
        .mutations
        .push(put("native-root-a", "second", 1, None));
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut connection = sqlx::SqliteConnection::connect(&format!(
            "sqlite://{}?mode=rw",
            fixture.path.display()
        ))
        .await
        .unwrap();
        sqlx::query("UPDATE retention_lock SET epoch=? WHERE singleton=1")
            .bind(i64::MAX - 1)
            .execute(&mut connection)
            .await
            .unwrap();
        connection.close().await.unwrap();
    });
    let before = observed(store, &request);
    assert_eq!(
        store.mutate_root_provider_states(request.clone()),
        Err(StoreError::CapacityExceeded)
    );
    assert_eq!(observed(store, &request), before);
}
