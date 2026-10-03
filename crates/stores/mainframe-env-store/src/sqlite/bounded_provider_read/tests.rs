use super::*;
use crate::provider_scan::tests::{OwnedDirectory, pages, record};

fn raw(store: &SqliteStateStore, sql: &'static str) {
    store.run(sqlx::raw_sql(sql).execute(&store.pool)).unwrap();
}

// Inspect every physical table/cell, including epoch, clock, journal and retention.
// Fixture identity only; this does not invent an application codec or authority.
fn footprint(store: &SqliteStateStore) -> Vec<(String, Vec<Vec<Option<Vec<u8>>>>)> {
    let tables = store
        .run(
            sqlx::query("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
                .fetch_all(&store.pool),
        )
        .unwrap();
    tables
        .into_iter()
        .map(|table| {
            let name: String = table.get(0);
            let escaped = name.replace('"', "\"\"");
            let columns = store
                .run(
                    sqlx::query(sqlx::AssertSqlSafe(format!(
                        "PRAGMA table_info(\"{escaped}\")"
                    )))
                    .fetch_all(&store.pool),
                )
                .unwrap();
            let projection = columns
                .iter()
                .map(|column| {
                    let field: String = column.get(1);
                    format!("CAST(\"{}\" AS BLOB)", field.replace('"', "\"\""))
                })
                .collect::<Vec<_>>()
                .join(",");
            let rows = store
                .run(
                    sqlx::query(sqlx::AssertSqlSafe(format!(
                        "SELECT {projection} FROM \"{escaped}\" ORDER BY rowid"
                    )))
                    .fetch_all(&store.pool),
                )
                .unwrap();
            (
                name,
                rows.into_iter()
                    .map(|row| {
                        (0..columns.len())
                            .map(|i| row.get::<Option<Vec<u8>>, _>(i))
                            .collect()
                    })
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn bounded_sqlite_pages_legacy_parity_and_owned_reopen() {
    let directory = OwnedDirectory::create();
    let store = SqliteStateStore::open(&directory.url(), 2048, 20).unwrap();
    pages(&store);
    let before = footprint(&store);
    drop(store);
    let reopened =
        SqliteStateStore::open(&directory.url().replace("mode=rwc", "mode=rw"), 2048, 20).unwrap();
    assert_eq!(footprint(&reopened), before);
    assert_eq!(
        reopened.list_provider_state_bounded("page", 4, 29).unwrap(),
        [
            record("a", b"first"),
            record("z", b"last"),
            record("é", b"utf8")
        ]
    );
    assert_eq!(footprint(&reopened), before);
    drop(reopened);
}

#[test]
fn bounded_sqlite_all_physical_rows_clock_and_epoch_unchanged() {
    let directory = OwnedDirectory::create();
    let store = SqliteStateStore::open(&directory.url(), 2048, 20).unwrap();
    store
        .put_provider_state(record("a", b"first"), None)
        .unwrap();
    store.advance_logical_clock(99).unwrap();
    let before = footprint(&store);
    assert_eq!(
        store.list_provider_state_bounded("page", 2, 10).unwrap(),
        [record("a", b"first")]
    );
    assert_eq!(
        store.list_provider_state_bounded("page", 2, 9),
        Err(StoreError::CapacityExceeded)
    );
    assert_eq!(footprint(&store), before);
    drop(store);
}

#[test]
fn bounded_sqlite_shape_and_utf8_preflight_refuses_no_partial_page() {
    for (sql, expected) in [
        (
            "INSERT INTO provider_state VALUES('page','z','invalid',x'0001')",
            StoreError::IncompatibleVersion,
        ),
        (
            "PRAGMA ignore_check_constraints=ON; INSERT INTO provider_state VALUES('page','z',0,x'0001')",
            StoreError::IncompatibleVersion,
        ),
        (
            "INSERT INTO provider_state VALUES('page','',1,x'0001')",
            StoreError::IncompatibleVersion,
        ),
        (
            "INSERT INTO provider_state VALUES('page',CAST(x'ff' AS TEXT),1,x'0001')",
            StoreError::IncompatibleVersion,
        ),
        (
            "INSERT INTO provider_state VALUES('page','z',1,'not-a-blob')",
            StoreError::IncompatibleVersion,
        ),
        (
            "INSERT INTO provider_state VALUES('page','z',1,zeroblob(9))",
            StoreError::PayloadTooLarge,
        ),
        (
            "INSERT INTO provider_state VALUES('page',printf('%.*c',1025,'z'),1,x'01')",
            StoreError::IncompatibleVersion,
        ),
    ] {
        let directory = OwnedDirectory::create();
        let store = SqliteStateStore::open(&directory.url(), 8, 20).unwrap();
        store
            .put_provider_state(record("a", b"good"), None)
            .unwrap();
        raw(&store, sql);
        let before = footprint(&store);
        assert_eq!(
            store.list_provider_state_bounded("page", 3, 2048),
            Err(expected),
            "{sql}"
        );
        assert_eq!(footprint(&store), before);
        drop(store);
    }
}

#[test]
fn bounded_sqlite_byte_refusal_precedes_blob_and_version_decode() {
    let directory = OwnedDirectory::create();
    let store = SqliteStateStore::open(&directory.url(), 8, 20).unwrap();
    raw(
        &store,
        "INSERT INTO provider_state VALUES('page','a','invalid',zeroblob(32))",
    );
    let before = footprint(&store);
    // A payload fetch/decode would produce a SQL type error on the corrupt version.
    // The limited metadata page instead refuses its 37 bytes before either fetch.
    assert_eq!(
        store.list_provider_state_bounded("page", 1, 36),
        Err(StoreError::CapacityExceeded)
    );
    assert_eq!(
        store.list_provider_state_bounded("page", 1, 37),
        Err(StoreError::IncompatibleVersion)
    );
    assert_eq!(footprint(&store), before);
    drop(store);
}
