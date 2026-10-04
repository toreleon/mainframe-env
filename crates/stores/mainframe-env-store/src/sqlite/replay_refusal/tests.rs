use super::*;
use crate::replay_refusal::tests::request;
use mainframe_env_store_api::JournalStore;
fn raw(store: &SqliteStateStore) -> (Vec<(String, String, i64, Vec<u8>)>, (i64, i64)) {
    (store.run(sqlx::query_as("SELECT namespace,key,version,CAST(payload AS BLOB) FROM provider_state ORDER BY namespace,key").fetch_all(&store.pool)).unwrap(),
    store.run(sqlx::query_as("SELECT epoch,clock_tick FROM retention_lock WHERE singleton=1").fetch_one(&store.pool)).unwrap())
}
#[test]
fn checked_replay_refusal_sqlite_late_audit_outbox_quota_epoch_faults_rollback_whole_writer() {
    for fault in 0..5 {
        let mut s = SqliteStateStore::open("sqlite::memory:", 8192, 100).unwrap();
        let r = request(&s);
        match fault {
            0 => {
                s.run(sqlx::query("CREATE TRIGGER refuse_audit BEFORE INSERT ON provider_state WHEN NEW.namespace='durable-audit-v1' BEGIN SELECT RAISE(ABORT,'late audit'); END").execute(&s.pool)).unwrap();
            }
            1 => {
                s.run(sqlx::query("CREATE TRIGGER refuse_outbox BEFORE INSERT ON provider_state WHEN NEW.namespace='durable-outbox' BEGIN SELECT RAISE(ABORT,'late outbox'); END").execute(&s.pool)).unwrap();
            }
            2 => s.max_rows = 7,
            3 => {
                s.run(
                    sqlx::query("UPDATE retention_lock SET epoch=?")
                        .bind(i64::MAX - 1)
                        .execute(&s.pool),
                )
                .unwrap();
            }
            4 => {
                s.run(sqlx::query("UPDATE retention_lock SET clock_tick=11").execute(&s.pool))
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let before = raw(&s);
        assert!(s.commit_checked_replay_refusal(r).is_err());
        assert_eq!(raw(&s), before, "fault {fault}");
    }
}
#[test]
fn checked_replay_refusal_sqlite_bounded_core_and_keyset_phantoms_refuse_without_writes() {
    for fault in 0..4 {
        let s = SqliteStateStore::open("sqlite::memory:", 8192, 100).unwrap();
        let r = request(&s);
        match fault {
            0 => {
                s.run(sqlx::query("UPDATE provider_state SET payload=zeroblob(8193) WHERE namespace='durable-effect'").execute(&s.pool)).unwrap();
            }
            1 => {
                s.run(sqlx::query("UPDATE provider_state SET key='00000000000000000000' WHERE namespace LIKE 'durable-event:%' AND key='00000000000000000001'").execute(&s.pool)).unwrap();
            }
            2 => {
                s.run(sqlx::query("INSERT INTO provider_state(namespace,key,version,payload) SELECT namespace,'00000000000000000005',1,payload FROM provider_state WHERE namespace LIKE 'durable-event:%' LIMIT 1").execute(&s.pool)).unwrap();
            }
            3 => {
                s.run(sqlx::query("UPDATE provider_state SET key=? WHERE namespace LIKE 'durable-event:%' AND key='00000000000000000001'").bind("1".repeat(1025)).execute(&s.pool)).unwrap();
            }
            _ => unreachable!(),
        }
        let before = raw(&s);
        assert!(s.commit_checked_replay_refusal(r).is_err());
        assert_eq!(raw(&s), before);
    }
}
