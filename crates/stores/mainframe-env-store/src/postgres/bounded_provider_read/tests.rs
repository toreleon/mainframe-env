use super::*;
use crate::provider_scan::tests::{pages, record};

fn store(max_payload_bytes: usize) -> PostgresStateStore {
    let url = std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL")
        .expect("explicit isolated PostgreSQL test URL required");
    PostgresStateStore::open(&url, max_payload_bytes, 262_144).unwrap()
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
fn bounded_postgres_pages_bytes_order_and_read_only() {
    let store = store(1024 * 1024);
    // The unchanged Memory/SQLite oracle covers UTF-8 costs, exact/one-below
    // boundaries, page truncation, global maxima, legacy parity and no clock/epoch writes.
    pages(&store);
    block_on(&store.runtime, async {
        let mut transaction = store.begin_provider_snapshot().await.unwrap();
        let settings: (String, String) = sqlx::query_as(
            "SELECT current_setting('transaction_isolation'),\
             current_setting('transaction_read_only')",
        )
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
        assert_eq!(settings, ("repeatable read".into(), "on".into()));
        transaction.rollback().await.unwrap();
        Ok::<_, StoreError>(())
    })
    .unwrap()
    .unwrap();
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
fn bounded_postgres_physical_corruption_and_preflight_refuse_complete_page() {
    let store = store(8);
    block_on(&store.runtime, async {
        for (sql, expected) in [
            (
                "INSERT INTO pg_temp.provider_state VALUES('page','z',0,decode('0001','hex'))",
                StoreError::IncompatibleVersion,
            ),
            (
                "INSERT INTO pg_temp.provider_state VALUES('page','z',-1,decode('0001','hex'))",
                StoreError::IncompatibleVersion,
            ),
            (
                "INSERT INTO pg_temp.provider_state VALUES('page','',1,decode('0001','hex'))",
                StoreError::IncompatibleVersion,
            ),
            (
                "INSERT INTO pg_temp.provider_state VALUES('page',repeat('z',1025),1,decode('01','hex'))",
                StoreError::IncompatibleVersion,
            ),
            (
                "INSERT INTO pg_temp.provider_state VALUES('page','z',1,decode(repeat('00',9),'hex'))",
                StoreError::PayloadTooLarge,
            ),
            (
                "INSERT INTO pg_temp.provider_state VALUES('page',NULL,1,decode('01','hex'))",
                StoreError::IncompatibleVersion,
            ),
            (
                "INSERT INTO pg_temp.provider_state VALUES('page','z',NULL,decode('01','hex'))",
                StoreError::IncompatibleVersion,
            ),
            (
                "INSERT INTO pg_temp.provider_state VALUES('page','z',1,NULL)",
                StoreError::IncompatibleVersion,
            ),
            (
                "INSERT INTO pg_temp.provider_state VALUES('page','a',1,decode('01','hex'))",
                StoreError::IncompatibleVersion,
            ),
            (
                "ALTER TABLE pg_temp.provider_state ALTER COLUMN payload TYPE TEXT \
                 USING convert_from(payload,'UTF8'); \
                 INSERT INTO pg_temp.provider_state VALUES('page','z',1,'bad')",
                StoreError::IncompatibleVersion,
            ),
        ] {
            let mut transaction = store.pool.begin().await.unwrap();
            // Only transaction-owned temporary physical fixtures bypass normal write
            // validation. No production schema or unrelated provider row is changed.
            sqlx::raw_sql(
                "CREATE TEMP TABLE provider_state \
                 (namespace TEXT,key TEXT,version BIGINT,payload BYTEA) ON COMMIT DROP; \
                 INSERT INTO pg_temp.provider_state VALUES('page','a',1,decode('676f6f64','hex'))",
            )
            .execute(&mut *transaction)
            .await
            .unwrap();
            sqlx::raw_sql(sql)
                .execute(&mut *transaction)
                .await
                .unwrap();
            let before: Vec<String> = sqlx::query_scalar(
                "SELECT to_jsonb(snapshot_row)::text FROM pg_temp.provider_state AS snapshot_row \
                 ORDER BY key COLLATE \"C\",to_jsonb(snapshot_row)::text COLLATE \"C\"",
            )
            .fetch_all(&mut *transaction)
            .await
            .unwrap();
            assert_eq!(
                store
                    .provider_page_in_snapshot(&mut transaction, "page", 3, 2048)
                    .await,
                Err(expected),
                "{sql}"
            );
            let after: Vec<String> = sqlx::query_scalar(
                "SELECT to_jsonb(snapshot_row)::text FROM pg_temp.provider_state AS snapshot_row \
                 ORDER BY key COLLATE \"C\",to_jsonb(snapshot_row)::text COLLATE \"C\"",
            )
            .fetch_all(&mut *transaction)
            .await
            .unwrap();
            assert_eq!(after, before);
            transaction.rollback().await.unwrap();
        }
        let mut transaction = store.pool.begin().await.unwrap();
        sqlx::raw_sql(
            "CREATE TEMP TABLE provider_state \
             (namespace TEXT,key TEXT,version BIGINT,payload BYTEA) ON COMMIT DROP; \
             INSERT INTO pg_temp.provider_state VALUES\
             ('page','a',0,decode(repeat('00',32),'hex'))",
        )
        .execute(&mut *transaction)
        .await
        .unwrap();
        // The 37-byte page refuses its byte budget before the invalid version.
        assert_eq!(
            store
                .provider_page_in_snapshot(&mut transaction, "page", 1, 36)
                .await,
            Err(StoreError::CapacityExceeded)
        );
        assert_eq!(
            store
                .provider_page_in_snapshot(&mut transaction, "page", 1, 37)
                .await,
            Err(StoreError::IncompatibleVersion)
        );
        sqlx::query("DELETE FROM pg_temp.provider_state")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::raw_sql(
            "INSERT INTO pg_temp.provider_state VALUES\
             ('page','a',1,decode('676f6f64','hex')),('page','z',0,decode('01','hex'))",
        )
        .execute(&mut *transaction)
        .await
        .unwrap();
        // A later corrupt row cannot turn a valid first page into a whole-namespace claim.
        assert_eq!(
            store
                .provider_page_in_snapshot(&mut transaction, "page", 1, 9)
                .await
                .unwrap(),
            [record("a", b"good")]
        );
        assert_eq!(
            store
                .provider_page_in_snapshot(&mut transaction, "page", 2, 16)
                .await,
            Err(StoreError::IncompatibleVersion)
        );
        transaction.rollback().await.unwrap();
        Ok::<_, StoreError>(())
    })
    .unwrap()
    .unwrap();
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
fn bounded_postgres_repeatable_snapshot_ignores_committed_changes() {
    let store = store(1024 * 1024);
    let namespace = "bounded-snapshot";
    let old = vec![
        ProviderStateRecord {
            namespace: namespace.into(),
            key: "a".into(),
            version: 1,
            payload: b"old-a".to_vec(),
        },
        ProviderStateRecord {
            namespace: namespace.into(),
            key: "b".into(),
            version: 1,
            payload: b"old-b".to_vec(),
        },
    ];
    for row in &old {
        store.put_provider_state(row.clone(), None).unwrap();
    }
    let observed = block_on(&store.runtime, async {
        let mut reader = store.begin_provider_snapshot().await.unwrap();
        // Establish this actual read snapshot before committing a different connection.
        let keys: Vec<String> = sqlx::query_scalar(
            "SELECT key FROM provider_state WHERE namespace=$1 ORDER BY key COLLATE \"C\"",
        )
        .bind(namespace)
        .fetch_all(&mut *reader)
        .await
        .unwrap();
        assert_eq!(keys, ["a", "b"]);
        let reader_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *reader)
            .await
            .unwrap();
        let mut writer = store.pool.begin().await.unwrap();
        let writer_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *writer)
            .await
            .unwrap();
        assert_ne!(writer_pid, reader_pid);
        assert_eq!(
            sqlx::query(
                "UPDATE provider_state SET version=2,payload=$2 WHERE namespace=$1 AND key='a'",
            )
            .bind(namespace)
            .bind(b"new-a".as_slice())
            .execute(&mut *writer)
            .await
            .unwrap()
            .rows_affected(),
            1
        );
        assert_eq!(
            sqlx::query("DELETE FROM provider_state WHERE namespace=$1 AND key='b'")
                .bind(namespace)
                .execute(&mut *writer)
                .await
                .unwrap()
                .rows_affected(),
            1
        );
        sqlx::query(
            "INSERT INTO provider_state(namespace,key,version,payload) VALUES($1,'c',1,$2)",
        )
        .bind(namespace)
        .bind(b"new-c".as_slice())
        .execute(&mut *writer)
        .await
        .unwrap();
        writer.commit().await.unwrap();
        // Every metadata/key/payload phase still reads the original committed page.
        let page = store
            .provider_page_in_snapshot(&mut reader, namespace, 3, 1024)
            .await
            .unwrap();
        reader.rollback().await.unwrap();
        Ok::<_, StoreError>(page)
    })
    .unwrap()
    .unwrap();
    assert_eq!(observed, old);
    assert_eq!(
        store
            .list_provider_state_bounded(namespace, 3, 1024)
            .unwrap(),
        [
            ProviderStateRecord {
                namespace: namespace.into(),
                key: "a".into(),
                version: 2,
                payload: b"new-a".to_vec(),
            },
            ProviderStateRecord {
                namespace: namespace.into(),
                key: "c".into(),
                version: 1,
                payload: b"new-c".to_vec(),
            },
        ]
    );
}
