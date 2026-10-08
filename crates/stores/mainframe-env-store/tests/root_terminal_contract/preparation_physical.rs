//! Owned SQLite adversarial fixtures; they do not relax producer state machines.
use super::*;
use sqlx::Connection;

fn execute(fixture: &OwnedSqlite, sql: &'static str, namespace: &str, key: &str, payload: Vec<u8>) {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let mut connection = sqlx::SqliteConnection::connect(&format!(
            "sqlite://{}?mode=rw",
            fixture.path.display()
        ))
        .await
        .unwrap();
        sqlx::query(sql)
            .bind(namespace)
            .bind(key)
            .bind(payload)
            .execute(&mut connection)
            .await
            .unwrap();
        connection.close().await.unwrap();
    });
}
#[test]
fn sqlite_preparation_exact_initial_indexes_and_foreign_overlaps_refuse() {
    for fault in 0..9 {
        let fixture = OwnedSqlite::new();
        let input = request(fixture.store());
        let (namespace, key) = match fault {
            0 => ("durable-root-actor-v1", "native-root"),
            1 => ("durable-root-run-v1", "native-run"),
            2 | 3 => ("durable-root-scope-v1", "mq-test-terminal"),
            4 => ("durable-root-scope-v1", "native-root-owned-test"),
            5 => ("durable-root-row-scope-v1", "exact-preparation:old"),
            6 => ("durable-root-actor-v1", "phantom-child"),
            7 => ("durable-root-row-scope-v1", "foreign-overlap"),
            _ => ("durable-root-driver-v1", "native-root"),
        };
        // Use actual stored keys for exact-row/driver identities, never guessed framing.
        let rows = fixture
            .store()
            .list_provider_state_prefix("durable-root-", 64)
            .unwrap();
        let key = if fault == 5 {
            rows.iter()
                .find(|r| r.namespace == namespace)
                .unwrap()
                .key
                .as_str()
        } else if fault == 8 {
            input.claim.inserted_row().key.as_str()
        } else {
            key
        };
        let namespace = if fault == 8 {
            ROOT_DRIVER_NAMESPACE
        } else {
            namespace
        };
        let sql = match fault {
            0..=2 | 5 => "DELETE FROM provider_state WHERE namespace=? AND key=? AND length(?)>=0",
            6 | 7 => "INSERT INTO provider_state(namespace,key,version,payload) VALUES(?,?,1,?)",
            _ => "UPDATE provider_state SET payload=?3 WHERE namespace=?1 AND key=?2",
        };
        let bytes = if fault == 3 || fault == 4 {
            b"foreign-root".to_vec()
        } else if fault == 8 {
            b"malformed".to_vec()
        } else {
            b"native-root".to_vec()
        };
        execute(&fixture, sql, namespace, key, bytes);
        // Malformed root cannot be decoded by audit queries; compare physical state directly.
        if fault == 8 {
            let before = raw_snapshot(&fixture);
            assert!(
                fixture
                    .store()
                    .mutate_root_preparation_states(input)
                    .is_err()
            );
            assert_eq!(raw_snapshot(&fixture), before);
        } else {
            refuses(fixture.store(), &input, input.clone());
        }
    }
}
type RawSnapshot = (Vec<(String, String, i64, Vec<u8>)>, (i64, i64));

fn raw_snapshot(fixture: &OwnedSqlite) -> RawSnapshot {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let mut connection = sqlx::SqliteConnection::connect(&format!(
            "sqlite://{}?mode=rw",
            fixture.path.display()
        ))
        .await
        .unwrap();
        let rows = sqlx::query_as(
            "SELECT namespace,key,version,payload FROM provider_state ORDER BY namespace,key",
        )
        .fetch_all(&mut connection)
        .await
        .unwrap();
        let counters =
            sqlx::query_as("SELECT epoch,clock_tick FROM retention_lock WHERE singleton=1")
                .fetch_one(&mut connection)
                .await
                .unwrap();
        connection.close().await.unwrap();
        (rows, counters)
    })
}
#[test]
fn sqlite_preparation_adversarial_effect_work_checkpoint_event_refuse_without_any_change() {
    for fault in 0..6 {
        let fixture = OwnedSqlite::new();
        let input = request(fixture.store());
        let (namespace, key, bytes) = match fault {
            0..=2 => {
                let mut effect = orphan_intent();
                effect.execution_id = input.execution.execution_id.clone();
                effect.run_unit_id = input.execution.run_unit_id.clone();
                effect.intent.owner = effect.execution_id.clone();
                // Take the existing codec's exact body from a separate valid legacy fixture.
                let legacy = MemoryStore::new(StoreLimits::default());
                legacy.record_intent(effect.clone()).unwrap();
                let isolated = OwnedSqlite::new();
                isolated.store().record_intent(effect.clone()).unwrap();
                let row = isolated
                    .store()
                    .get_provider_state("durable-effect", effect.key.as_str())
                    .unwrap()
                    .unwrap();
                let mut value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
                if fault == 1 {
                    value["state"] = "unknown-outcome".into();
                }
                if fault == 2 {
                    value["intent"]["epoch"] = 2.into();
                }
                (
                    "durable-effect".to_string(),
                    effect.key.as_str().to_string(),
                    serde_json::to_vec(&value).unwrap(),
                )
            }
            3 => (
                "durable-work".into(),
                "work".into(),
                br#"{"execution":"native-root"}"#.to_vec(),
            ),
            4 => (
                "durable-checkpoint".into(),
                "native-root".into(),
                b"adversarial-checkpoint".to_vec(),
            ),
            _ => (
                "durable-event:native-root".into(),
                format!("{:020}", 2),
                b"adversarial-extra-event".to_vec(),
            ),
        };
        execute(
            &fixture,
            "INSERT INTO provider_state(namespace,key,version,payload) VALUES(?,?,1,?)",
            &namespace,
            &key,
            bytes,
        );
        let before = raw_snapshot(&fixture);
        assert!(
            fixture
                .store()
                .mutate_root_preparation_states(input)
                .is_err()
        );
        assert_eq!(raw_snapshot(&fixture), before);
    }
}
#[test]
fn sqlite_preparation_epoch_exhaustion_and_late_clock_failure_whole_rollback() {
    for clock_failure in [false, true] {
        let fixture = OwnedSqlite::new();
        let mut input = request(fixture.store());
        input
            .mutations
            .push(attributed::put("native-root-owned-test", "second", 1, None));
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let mut connection = sqlx::SqliteConnection::connect(&format!("sqlite://{}?mode=rw", fixture.path.display())).await.unwrap();
            if clock_failure { sqlx::query("CREATE TRIGGER fail_preparation_clock BEFORE UPDATE OF clock_tick ON retention_lock WHEN NEW.clock_tick>10 BEGIN SELECT RAISE(ABORT,'owned late clock fault'); END").execute(&mut connection).await.unwrap(); }
            else { sqlx::query("UPDATE retention_lock SET epoch=? WHERE singleton=1").bind(i64::MAX-1).execute(&mut connection).await.unwrap(); }
            connection.close().await.unwrap();
        });
        let before = raw_snapshot(&fixture);
        assert!(
            fixture
                .store()
                .mutate_root_preparation_states(input)
                .is_err()
        );
        assert_eq!(raw_snapshot(&fixture), before);
    }
}
