use mainframe_env_execution_api::{ArtifactRef, InvocationLimits};
use mainframe_env_store::{PostgresArtifactStore, PostgresStateStore};
use mainframe_env_store_api::{
    ArtifactRecord, ArtifactStore, ProviderStateMutation, ProviderStateRecord, ProviderStateStore,
    ProviderStateWrite, StoreError,
};
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use std::sync::{Arc, Barrier};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn artifact(payload: &[u8], media_type: &str) -> ArtifactRecord {
    let payload = payload.to_vec();
    let payload_digest: [u8; 32] = Sha256::digest(&payload).into();
    ArtifactRecord {
        artifact: ArtifactRef::new(
            format!("sha256:{}", hex(&payload_digest)),
            InvocationLimits::default(),
        )
        .unwrap(),
        media_type: media_type.into(),
        payload_digest,
        payload,
    }
}

fn state(key: impl Into<String>) -> ProviderStateRecord {
    ProviderStateRecord {
        namespace: "r19-quota".into(),
        key: key.into(),
        version: 1,
        payload: b"bounded".to_vec(),
    }
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
fn postgres_quota_and_shared_artifact_contract() {
    let url = std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL")
        .expect("explicit PostgreSQL test URL required");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let pool = runtime
        .block_on(PgPoolOptions::new().max_connections(2).connect(&url))
        .unwrap();
    let server_version: i32 = runtime
        .block_on(
            sqlx::query_scalar("SELECT current_setting('server_version_num')::integer")
                .fetch_one(&pool),
        )
        .unwrap();
    assert_eq!(server_version / 10_000, 18);

    let stores = (0..32)
        .map(|_| Arc::new(PostgresStateStore::open(&url, 1024, 8).unwrap()))
        .collect::<Vec<_>>();
    let barrier = Arc::new(Barrier::new(33));
    let workers = (0..32)
        .map(|index| {
            let store = stores[index].clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                (
                    index,
                    store.put_provider_state(state(format!("row-{index:02}")), None),
                )
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let outcomes = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    let inserted = outcomes
        .iter()
        .filter_map(|(index, result)| result.is_ok().then_some(*index))
        .collect::<Vec<_>>();
    assert_eq!(inserted.len(), 8);
    assert_eq!(
        outcomes
            .iter()
            .filter(|(_, result)| *result == Err(StoreError::CapacityExceeded))
            .count(),
        24
    );
    assert_eq!(
        stores[0].list_provider_state("r19-quota", 8).unwrap().len(),
        8
    );
    assert!(matches!(
        PostgresStateStore::open(&url, 1024, 9),
        Err(StoreError::IncompatibleVersion)
    ));

    let removed = format!("row-{:02}", inserted[0]);
    stores[0]
        .delete_provider_state("r19-quota", &removed, 1)
        .unwrap();
    stores[1]
        .put_provider_state(state("replacement"), None)
        .unwrap();
    let retained = format!("row-{:02}", inserted[1]);
    assert_eq!(
        stores[2].mutate_provider_states_atomic(vec![
            ProviderStateMutation::Delete {
                namespace: "r19-quota".into(),
                key: retained.clone(),
                expected_version: 1,
            },
            ProviderStateMutation::Put(ProviderStateWrite {
                record: state("overflow-a"),
                expected_version: None,
            }),
            ProviderStateMutation::Put(ProviderStateWrite {
                record: state("overflow-b"),
                expected_version: None,
            }),
        ]),
        Err(StoreError::CapacityExceeded)
    );
    assert!(
        stores[0]
            .get_provider_state("r19-quota", &retained)
            .unwrap()
            .is_some()
    );
    assert!(
        stores[0]
            .get_provider_state("r19-quota", "overflow-a")
            .unwrap()
            .is_none()
    );

    let left = Arc::new(PostgresArtifactStore::open(&url, 1024, 2).unwrap());
    let right = Arc::new(PostgresArtifactStore::open(&url, 1024, 2).unwrap());
    let left_record = artifact(b"shared", "application/x-left");
    let right_record = artifact(b"shared", "application/x-right");
    let shared_id = left_record.artifact.clone();
    let barrier = Arc::new(Barrier::new(3));
    let left_worker = {
        let store = left.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            store.put_artifact(left_record)
        })
    };
    let right_worker = {
        let store = right.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            store.put_artifact(right_record)
        })
    };
    barrier.wait();
    let artifact_results = [left_worker.join().unwrap(), right_worker.join().unwrap()];
    assert_eq!(
        artifact_results
            .iter()
            .filter(|result| result.is_ok())
            .count(),
        1
    );
    assert_eq!(
        artifact_results
            .iter()
            .filter(|result| **result == Err(StoreError::Conflict))
            .count(),
        1
    );
    let winner = left.get_artifact(&shared_id).unwrap().unwrap();
    assert_eq!(right.get_artifact(&shared_id).unwrap(), Some(winner));

    let crash_record = artifact(b"crash rollback", "application/octet-stream");
    runtime.block_on(async {
        let mut transaction = pool.begin().await.unwrap();
        sqlx::query("UPDATE store_quota SET used_rows=used_rows+1 WHERE quota_key='artifact-object'")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO artifact_object(object_key,schema_version,media_type,payload_digest,payload) VALUES($1,1,$2,$3,$4)",
        )
        .bind(crash_record.artifact.as_str())
        .bind(&crash_record.media_type)
        .bind(crash_record.payload_digest.as_slice())
        .bind(&crash_record.payload)
        .execute(&mut *transaction)
        .await
        .unwrap();
        transaction.rollback().await.unwrap();
    });
    assert_eq!(left.get_artifact(&crash_record.artifact).unwrap(), None);
    right.put_artifact(crash_record.clone()).unwrap();
    drop(left);
    drop(right);
    let reopened = PostgresArtifactStore::open(&url, 1024, 2).unwrap();
    assert_eq!(
        reopened.get_artifact(&crash_record.artifact).unwrap(),
        Some(crash_record)
    );
    assert!(reopened.is_ready());

    runtime
        .block_on(
            sqlx::query("UPDATE artifact_object SET payload=$1 WHERE object_key=$2")
                .bind(b"corrupt".as_slice())
                .bind(shared_id.as_str())
                .execute(&pool),
        )
        .unwrap();
    assert_eq!(
        reopened.get_artifact(&shared_id),
        Err(StoreError::IncompatibleVersion)
    );
    runtime
        .block_on(
            sqlx::query("UPDATE store_quota SET used_rows=0 WHERE quota_key='artifact-object'")
                .execute(&pool),
        )
        .unwrap();
    assert!(matches!(
        PostgresArtifactStore::open(&url, 1024, 2),
        Err(StoreError::IncompatibleVersion)
    ));
}
