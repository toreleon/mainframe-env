//! Genuine isolated PostgreSQL store contracts; no guest/subsystem acceptance credit.
use super::*;
use crate::publication::tests::{prepare, put};
use mainframe_env_execution_api::{
    AuditDecision, IdempotencyKey, InvocationLimits, LifecycleEvent, LifecycleEventKind,
};
use mainframe_env_store_api::{
    AuditSink, EffectState, ExecutionState, ExecutionStore, IdempotencyStore, JournalStore,
    OutboxRecord, ProviderStateIdentity, RootCustodySuccessor, RootDriverAdmission,
    RootProviderPublication, RootProviderRowAdmission,
};
use std::sync::{
    Arc, Barrier,
    atomic::{AtomicU64, Ordering},
};

struct OwnedPg {
    runtime: tokio::runtime::Runtime,
    pool: PgPool,
    schema: String,
    url: String,
}
impl OwnedPg {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let base = std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL")
            .expect("explicit isolated PostgreSQL test URL required");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let pool = runtime.block_on(PgPool::connect(&base)).unwrap();
        let version: i32 = runtime
            .block_on(
                sqlx::query_scalar("SELECT current_setting('server_version_num')::integer")
                    .fetch_one(&pool),
            )
            .unwrap();
        assert_eq!(version / 10_000, 18);
        let schema = format!(
            "checked_read_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        runtime
            .block_on(
                sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}"))).execute(&pool),
            )
            .unwrap();
        let separator = if base.contains('?') { '&' } else { '?' };
        let url = format!("{base}{separator}options=-csearch_path%3D{schema}");
        Self {
            runtime,
            pool,
            schema,
            url,
        }
    }
    fn store(&self) -> PostgresStateStore {
        PostgresStateStore::open(&self.url, 8192, 32).unwrap()
    }
}
impl Drop for OwnedPg {
    fn drop(&mut self) {
        // The schema name is generated solely from fixed ASCII and integer identities.
        self.runtime
            .block_on(
                sqlx::query(sqlx::AssertSqlSafe(format!(
                    "DROP SCHEMA {} CASCADE",
                    self.schema
                )))
                .execute(&self.pool),
            )
            .unwrap();
        self.runtime.block_on(self.pool.close());
    }
}

fn request(store: &PostgresStateStore) -> CheckedProviderReadPublication {
    let mut publication = prepare(store);
    publication.mutations = vec![put("receipt", 1, None, b"full retained result")];
    store
        .mutate_provider_states_atomic(vec![put("café✓", 1, None, b"q\0ueue")])
        .unwrap();
    CheckedProviderReadPublication {
        execution: store
            .get_execution(&publication.intent.execution_id)
            .unwrap()
            .unwrap(),
        publication,
        dependencies: vec![
            TerminalRowDependency::Exact(
                store
                    .get_provider_state("publication-fixture-v1", "café✓")
                    .unwrap()
                    .unwrap(),
            ),
            TerminalRowDependency::Absent {
                namespace: "publication-fixture-v1".into(),
                key: "receipt".into(),
            },
        ],
    }
}
fn replay(
    store: &PostgresStateStore,
    r: &CheckedProviderReadPublication,
) -> ProviderReplayAssertion {
    ProviderReplayAssertion {
        effect: r.publication.intent.clone(),
        execution: r.execution.clone(),
        observed_tick: 10,
        receipt: ProviderStateIdentity {
            namespace: "publication-fixture-v1".into(),
            key: "receipt".into(),
        },
        dependencies: vec![
            r.dependencies[0].clone(),
            TerminalRowDependency::Exact(
                store
                    .get_provider_state("publication-fixture-v1", "receipt")
                    .unwrap()
                    .unwrap(),
            ),
        ],
    }
}
type RawSnapshot = (
    Vec<(String, String, i64, Vec<u8>)>,
    Vec<(String, i64, i64)>,
    (i64, i64),
);
fn raw(store: &PostgresStateStore) -> RawSnapshot {
    (
        store.run(sqlx::query_as("SELECT namespace,key,version,payload FROM provider_state ORDER BY convert_to(namespace,'UTF8'),convert_to(key,'UTF8')").fetch_all(&store.pool)).unwrap(),
        store.run(sqlx::query_as("SELECT quota_key,max_rows,used_rows FROM store_quota ORDER BY quota_key").fetch_all(&store.pool)).unwrap(),
        store.run(sqlx::query_as("SELECT epoch,clock_tick FROM retention_lock WHERE singleton=1").fetch_one(&store.pool)).unwrap(),
    )
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
fn checked_read_postgres_exact_publication_and_nonwriting_replay() {
    let fixture = OwnedPg::new();
    let store = fixture.store();
    let r = request(&store);
    let before = r.dependencies[0].clone();
    store.publish_provider_read_audited(r.clone()).unwrap();
    if let TerminalRowDependency::Exact(row) = before {
        assert_eq!(
            store.get_provider_state(&row.namespace, &row.key).unwrap(),
            Some(row)
        );
    }
    assert_eq!(
        store
            .audit_records(&r.execution.execution_id, 0, 8)
            .unwrap(),
        vec![r.publication.audit.clone()]
    );
    assert_eq!(
        store.effect(&r.publication.intent.key).unwrap(),
        Some(r.publication.intent.clone())
    );
    let mut retained = replay(&store, &r);
    let before = raw(&store);
    store.assert_provider_replay(retained.clone()).unwrap();
    assert_eq!(raw(&store), before);
    retained.effect.state = EffectState::Completed;
    retained.effect.result_digest = Some([0x81; 32]);
    retained.effect.resolved_tick = Some(10);
    store
        .record_result(&retained.effect.key, retained.effect.clone())
        .unwrap();
    let before = raw(&store);
    store.assert_provider_replay(retained.clone()).unwrap();
    assert_eq!(raw(&store), before);
    let mut completed = r;
    completed.publication.intent = retained.effect;
    assert!(store.publish_provider_read_audited(completed).is_err());
    assert_eq!(raw(&store), before);
    indexed_publication_and_replay();
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
fn checked_read_postgres_forgeries_and_audit_only_denial() {
    let fixture = OwnedPg::new();
    let store = fixture.store();
    let r = request(&store);
    let before = raw(&store);
    for fault in 0..8 {
        let mut bad = r.clone();
        match fault {
            0 => bad.execution.version += 1,
            1 => bad.execution.owner_lease = Some("foreign".into()),
            2 => bad.publication.intent.request_digest[0] ^= 1,
            3 => {
                if let TerminalRowDependency::Exact(row) = &mut bad.dependencies[0] {
                    row.payload[0] ^= 1;
                }
            }
            4 => bad.dependencies.push(bad.dependencies[0].clone()),
            5 => {
                bad.publication.observed_tick = 30;
                bad.publication.audit.observed_tick = 30;
            }
            6 => {
                bad.publication.audit.principal = mainframe_env_execution_api::PrincipalId::new(
                    "OTHER",
                    mainframe_env_execution_api::InvocationLimits::default(),
                )
                .unwrap()
            }
            7 => bad.publication.audit.decision = AuditDecision::Deny,
            _ => unreachable!(),
        }
        assert!(
            store.publish_provider_read_audited(bad).is_err(),
            "fault {fault}"
        );
        assert_eq!(raw(&store), before, "fault {fault}");
    }
    let mut denied = r;
    denied.publication.audit.decision = AuditDecision::Deny;
    denied.publication.mutations.clear();
    store.publish_provider_read_audited(denied.clone()).unwrap();
    assert!(
        store
            .get_provider_state("publication-fixture-v1", "receipt")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .audit_records(&denied.execution.execution_id, 0, 8)
            .unwrap(),
        vec![denied.publication.audit.clone()]
    );
    // An unindexed ordinary actor cannot borrow a foreign root's namespace index.
    store
        .put_provider_state(
            ProviderStateRecord {
                namespace: crate::root_terminal::SCOPE_NAMESPACE.into(),
                key: "publication-fixture-v1".into(),
                version: 1,
                payload: b"foreign-root".to_vec(),
            },
            None,
        )
        .unwrap();
    let before = raw(&store);
    assert!(store.publish_provider_read_audited(denied).is_err());
    assert_eq!(raw(&store), before);
    indexed_fences_refuse();
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
fn checked_read_postgres_two_connections_race_one_receipt_one_audit() {
    let fixture = OwnedPg::new();
    let left = Arc::new(fixture.store());
    let right = Arc::new(fixture.store());
    let r = request(&left);
    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = [left.clone(), right.clone()]
        .into_iter()
        .map(|store| {
            let r = r.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.publish_provider_read_audited(r)
            })
        })
        .collect();
    let outcomes: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        left.audit_records(&r.execution.execution_id, 0, 8).unwrap(),
        vec![r.publication.audit]
    );
    assert_eq!(
        left.get_provider_state("publication-fixture-v1", "receipt")
            .unwrap(),
        right
            .get_provider_state("publication-fixture-v1", "receipt")
            .unwrap()
    );
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
fn checked_read_postgres_late_faults_rollback_rows_quota_clock_and_epoch() {
    for fault in 0..5 {
        let fixture = OwnedPg::new();
        let store = fixture.store();
        let r = request(&store);
        match fault {
            0 => {
                store.run(sqlx::raw_sql("CREATE FUNCTION refuse_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.namespace='durable-audit-v1' THEN RAISE EXCEPTION 'owned late audit fault'; END IF; RETURN NEW; END; $$; CREATE TRIGGER refuse_audit BEFORE INSERT ON provider_state FOR EACH ROW EXECUTE FUNCTION refuse_audit();").execute(&store.pool)).unwrap();
            }
            1 => {
                store.run(sqlx::raw_sql("CREATE FUNCTION refuse_clock() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.clock_tick=9 THEN RAISE EXCEPTION 'owned late clock fault'; END IF; RETURN NEW; END; $$; CREATE TRIGGER refuse_clock BEFORE UPDATE ON retention_lock FOR EACH ROW EXECUTE FUNCTION refuse_clock();").execute(&store.pool)).unwrap();
            }
            2 => {
                let mut next = 0;
                while store.provider_state_usage().unwrap() < store.max_rows {
                    store
                        .put_provider_state(
                            ProviderStateRecord {
                                namespace: "fill".into(),
                                key: format!("{next}"),
                                version: 1,
                                payload: vec![0],
                            },
                            None,
                        )
                        .unwrap();
                    next += 1;
                }
            }
            3 => {
                store
                    .run(
                        sqlx::query("UPDATE retention_lock SET epoch=$1 WHERE singleton=1")
                            .bind(i64::MAX - 1)
                            .execute(&store.pool),
                    )
                    .unwrap();
            }
            4 => {
                let row = ProviderStateRecord {
                    namespace: crate::durable::AUDIT_NAMESPACE.into(),
                    key: crate::durable::audit_storage_key(
                        &r.execution.execution_id,
                        &format!("direct:{:020}", u64::MAX),
                    ),
                    version: 1,
                    payload: crate::durable::encode_audit(&r.publication.audit).unwrap(),
                };
                store.put_provider_state(row, None).unwrap();
            }
            _ => unreachable!(),
        }
        let before = raw(&store);
        assert!(
            store.publish_provider_read_audited(r).is_err(),
            "fault {fault}"
        );
        assert_eq!(raw(&store), before, "fault {fault}");
    }
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
fn checked_read_postgres_physical_byte_limits_and_shapes_precede_payload_fetch() {
    let fixture = OwnedPg::new();
    let store = fixture.store();
    block_on(&store.runtime, async {
        for (bytes, version, expected) in [(8193, 0, StoreError::PayloadTooLarge), (8, 0, StoreError::IncompatibleVersion)] {
            let mut tx = store.pool.begin().await.unwrap();
            sqlx::query("CREATE TEMP TABLE provider_state(namespace TEXT,key TEXT,version BIGINT,payload BYTEA) ON COMMIT DROP").execute(&mut *tx).await.unwrap();
            sqlx::query("INSERT INTO pg_temp.provider_state VALUES('byte-test','café✓',$1,$2)").bind(version).bind(vec![0_u8; bytes]).execute(&mut *tx).await.unwrap();
            let mut budget = Budget::new(0);
            assert_eq!(store.pg_checked_row(&mut tx, "byte-test", "café✓", &mut budget).await, Err(expected));
            tx.rollback().await.unwrap();
        }
        let mut tx = store.pool.begin().await.unwrap();
        sqlx::query("CREATE TEMP TABLE provider_state(namespace TEXT,key TEXT,version BIGINT,payload BYTEA) ON COMMIT DROP").execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO pg_temp.provider_state VALUES('byte-test','café✓',1,$1)").bind(b"q\0ueue".as_slice()).execute(&mut *tx).await.unwrap();
        let mut budget = Budget::new(mainframe_env_store_api::MAX_ROOT_PAYLOAD_BYTES - 1);
        assert_eq!(store.pg_checked_row(&mut tx, "byte-test", "café✓", &mut budget).await, Err(StoreError::CapacityExceeded));
        let mut budget = Budget::new(0);
        assert_eq!(store.pg_checked_row(&mut tx, "byte-test", "café✓", &mut budget).await.unwrap().unwrap().payload, b"q\0ueue");
        sqlx::query("INSERT INTO pg_temp.provider_state VALUES('byte-test','café✓',1,$1)").bind(b"duplicate".as_slice()).execute(&mut *tx).await.unwrap();
        let mut budget = Budget::new(0);
        assert_eq!(store.pg_checked_row(&mut tx, "byte-test", "café✓", &mut budget).await, Err(StoreError::IncompatibleVersion));
        tx.rollback().await.unwrap();
        // A physically different integer type is corruption, even with a legal
        // positive value and small payload. This table is rollback-owned only.
        let mut tx = store.pool.begin().await.unwrap();
        sqlx::query("CREATE TEMP TABLE provider_state(namespace TEXT,key TEXT,version INTEGER,payload BYTEA) ON COMMIT DROP").execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO pg_temp.provider_state VALUES('byte-test','café✓',1,$1)").bind(b"small".as_slice()).execute(&mut *tx).await.unwrap();
        let mut budget = Budget::new(0);
        assert_eq!(store.pg_checked_row(&mut tx, "byte-test", "café✓", &mut budget).await, Err(StoreError::IncompatibleVersion));
        tx.rollback().await.unwrap();
        Ok::<_, StoreError>(())
    }).unwrap().unwrap();
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
fn audited_postgres_move_delete_and_direct_audit_order() {
    let fixture = OwnedPg::new();
    let store = fixture.store();
    let mut publication = prepare(&store);
    store
        .mutate_provider_states_atomic(vec![
            put("old", 1, None, b"before"),
            put("delete", 1, None, b"delete"),
        ])
        .unwrap();
    publication.mutations = vec![
        ProviderStateMutation::Move {
            record: ProviderStateRecord {
                namespace: "publication-fixture-v1".into(),
                key: "new".into(),
                version: 2,
                payload: b"after".to_vec(),
            },
            old_key: "old".into(),
            expected_version: 1,
        },
        ProviderStateMutation::Delete {
            namespace: "publication-fixture-v1".into(),
            key: "delete".into(),
            expected_version: 1,
        },
    ];
    store
        .publish_provider_states_audited(publication.clone())
        .unwrap();
    assert!(
        store
            .get_provider_state("publication-fixture-v1", "old")
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .get_provider_state("publication-fixture-v1", "delete")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .get_provider_state("publication-fixture-v1", "new")
            .unwrap()
            .unwrap()
            .payload,
        b"after"
    );
    let mut denied = publication.clone();
    denied.mutations.clear();
    denied.audit.decision = AuditDecision::Deny;
    store
        .publish_provider_states_audited(denied.clone())
        .unwrap();
    let mut direct = publication.audit.clone();
    direct.decision = AuditDecision::Rejected;
    store.record_audit(direct.clone()).unwrap();
    assert_eq!(
        store
            .audit_records(&publication.intent.execution_id, 0, 8)
            .unwrap(),
        vec![publication.audit, denied.audit, direct]
    );
    successor_pin_mutations();
}

fn admission_for(r: &CheckedProviderReadPublication, row_scoped: bool) -> RootDriverAdmission {
    let mut execution = r.execution.clone();
    execution.state = ExecutionState::Admitted;
    execution.version = 1;
    let event = LifecycleEvent {
        execution_id: execution.execution_id.clone(),
        run_unit_id: execution.run_unit_id.clone(),
        sequence: 1,
        attempt: execution.attempt,
        tick: 5,
        kind: LifecycleEventKind::Admitted,
    };
    let notification = notification_for(&event);
    RootDriverAdmission {
        execution,
        event,
        notification,
        invocation_key: IdempotencyKey::new("pg-root-fixture", InvocationLimits::default())
            .unwrap(),
        configuration_digest: [5; 32],
        deadline_tick: 100,
        provider_namespaces: if row_scoped {
            vec![]
        } else {
            vec!["publication-fixture-v1".into()]
        },
        provider_rows: if row_scoped {
            ["café✓", "receipt"]
                .into_iter()
                .map(|key| ProviderStateIdentity {
                    namespace: "publication-fixture-v1".into(),
                    key: key.into(),
                })
                .collect()
        } else {
            vec![]
        },
    }
}
fn notification_for(event: &LifecycleEvent) -> OutboxRecord {
    OutboxRecord {
        notification_id: format!("{}:{:020}", event.execution_id, event.sequence),
        execution_id: event.execution_id.clone(),
        sequence: event.sequence,
        topic: "execution.lifecycle.v1".into(),
        payload: mainframe_env_execution_api::lifecycle_notification_payload(&event.kind),
        attempt: 0,
        delivered: false,
        delivered_tick: None,
        version: 1,
    }
}

// Genuine shared Memory admission creates the canonical ownership codec and
// indices. Copying its exact physical rows is isolated PG fixture setup only;
// PostgreSQL root admission/settlement remain unsupported and uncredited.
fn indexed_graph(store: &PostgresStateStore, r: &CheckedProviderReadPublication, row_scoped: bool) {
    let donor = crate::MemoryStore::new(crate::StoreLimits::default());
    donor
        .admit_root_driver(admission_for(r, row_scoped))
        .unwrap();
    let mut rows = Vec::new();
    for namespace in [
        ROOT_DRIVER_NAMESPACE,
        ACTOR_NAMESPACE,
        RUN_NAMESPACE,
        SCOPE_NAMESPACE,
        ROW_SCOPE_NAMESPACE,
    ] {
        rows.extend(donor.list_provider_state(namespace, 16).unwrap());
    }
    store
        .mutate_provider_states_atomic(
            rows.into_iter()
                .map(|record| {
                    ProviderStateMutation::Put(ProviderStateWrite {
                        record,
                        expected_version: None,
                    })
                })
                .collect(),
        )
        .unwrap();
}
fn indexed_publication_and_replay() {
    for row_scoped in [false, true] {
        let fixture = OwnedPg::new();
        let store = fixture.store();
        let r = request(&store);
        indexed_graph(&store, &r, row_scoped);
        let graph = store
            .list_provider_state_prefix("durable-root-", 16)
            .unwrap();
        store.publish_provider_read_audited(r.clone()).unwrap();
        assert_eq!(
            store
                .list_provider_state_prefix("durable-root-", 16)
                .unwrap(),
            graph
        );
        assert_eq!(
            store
                .audit_records(&r.execution.execution_id, 0, 8)
                .unwrap(),
            vec![r.publication.audit.clone()]
        );
        let mut assertion = replay(&store, &r);
        let before = raw(&store);
        store.assert_provider_replay(assertion.clone()).unwrap();
        assert_eq!(raw(&store), before);
        assertion.effect.state = EffectState::Completed;
        assertion.effect.result_digest = Some([0x81; 32]);
        assertion.effect.resolved_tick = Some(10);
        store
            .record_result(&assertion.effect.key, assertion.effect.clone())
            .unwrap();
        let before = raw(&store);
        store.assert_provider_replay(assertion).unwrap();
        assert_eq!(raw(&store), before);
    }
}
fn indexed_fences_refuse() {
    for row_scoped in [false, true] {
        for replaying in [false, true] {
            for fault in 0..11 {
                let fixture = OwnedPg::new();
                let store = fixture.store();
                let r = request(&store);
                indexed_graph(&store, &r, row_scoped);
                let assertion = if replaying {
                    store.publish_provider_read_audited(r.clone()).unwrap();
                    Some(replay(&store, &r))
                } else {
                    None
                };
                let scope = if row_scoped {
                    (
                        ROW_SCOPE_NAMESPACE,
                        row_scope_key("publication-fixture-v1", "café✓"),
                    )
                } else {
                    (SCOPE_NAMESPACE, "publication-fixture-v1".into())
                };
                let identity = match fault {
                    0 | 1 => (ACTOR_NAMESPACE, r.execution.execution_id.to_string()),
                    2 | 3 => (RUN_NAMESPACE, r.execution.run_unit_id.to_string()),
                    _ => scope.clone(),
                };
                match fault {
                    0 | 2 | 4 => {
                        store
                            .run(
                                sqlx::query(
                                    "DELETE FROM provider_state WHERE namespace=$1 AND key=$2",
                                )
                                .bind(identity.0)
                                .bind(&identity.1)
                                .execute(&store.pool),
                            )
                            .unwrap();
                    }
                    1 | 3 | 5 => {
                        store.run(sqlx::query("UPDATE provider_state SET payload=$1 WHERE namespace=$2 AND key=$3").bind(b"foreign-root".as_slice()).bind(identity.0).bind(&identity.1).execute(&store.pool)).unwrap();
                    }
                    6 => {
                        store.run(sqlx::query("UPDATE provider_state SET version=2 WHERE namespace=$1 AND key=$2").bind(scope.0).bind(&scope.1).execute(&store.pool)).unwrap();
                    }
                    7 => {
                        let (namespace, key) = if row_scoped {
                            (SCOPE_NAMESPACE, "publication-fixture-v1".into())
                        } else {
                            (
                                ROW_SCOPE_NAMESPACE,
                                row_scope_key("publication-fixture-v1", "café✓"),
                            )
                        };
                        store.run(sqlx::query("INSERT INTO provider_state(namespace,key,version,payload) VALUES($1,$2,1,$3)").bind(namespace).bind(key).bind(r.execution.execution_id.as_str().as_bytes()).execute(&store.pool)).unwrap();
                    }
                    8..=10 => {
                        let row = store
                            .get_provider_state(
                                ROOT_DRIVER_NAMESPACE,
                                r.execution.execution_id.as_str(),
                            )
                            .unwrap()
                            .unwrap();
                        let mut document = Document::read(&row).unwrap();
                        match fault {
                            8 => document.phase = crate::root_terminal::Phase::Closing,
                            9 => document.phase = crate::root_terminal::Phase::Uncertain,
                            10 => document.deadline = r.publication.observed_tick,
                            _ => unreachable!(),
                        }
                        let row = document.row(row.version + 1, 8192).unwrap();
                        store.run(sqlx::query("UPDATE provider_state SET version=$1,payload=$2 WHERE namespace=$3 AND key=$4").bind(row.version as i64).bind(row.payload).bind(row.namespace).bind(row.key).execute(&store.pool)).unwrap();
                    }
                    _ => unreachable!(),
                }
                let before = raw(&store);
                if let Some(assertion) = assertion {
                    assert!(
                        store.assert_provider_replay(assertion).is_err(),
                        "replay scope {row_scoped} fault {fault}"
                    );
                } else {
                    assert!(
                        store.publish_provider_read_audited(r.clone()).is_err(),
                        "scope {row_scoped} fault {fault}"
                    );
                }
                assert_eq!(raw(&store), before);
                if matches!(fault, 0..=3 | 8..=10) {
                    let mut audit_only = r.publication.clone();
                    audit_only.mutations.clear();
                    audit_only.audit.decision = AuditDecision::Deny;
                    assert!(
                        store.publish_provider_states_audited(audit_only).is_err(),
                        "audit scope {row_scoped} fault {fault}"
                    );
                    assert_eq!(raw(&store), before);
                }
            }
        }
    }
}

fn successor_pin_fixture(
    store: &PostgresStateStore,
    r: &CheckedProviderReadPublication,
) -> Vec<ProviderStateRecord> {
    let donor = crate::MemoryStore::new(crate::StoreLimits::default());
    let admission = admission_for(r, false);
    let claim = donor.admit_root_driver(admission.clone()).unwrap();
    let mut execution = admission.execution;
    for (sequence, state, tick, kind) in [
        (2, ExecutionState::Queued, 6, LifecycleEventKind::Queued),
        (3, ExecutionState::Running, 7, LifecycleEventKind::Started),
    ] {
        let event = LifecycleEvent {
            execution_id: execution.execution_id.clone(),
            run_unit_id: execution.run_unit_id.clone(),
            sequence,
            attempt: execution.attempt,
            tick,
            kind,
        };
        execution = donor
            .commit_execution_step(
                &execution.execution_id,
                execution.version,
                Some(state),
                event.clone(),
                None,
                None,
                None,
                notification_for(&event),
            )
            .unwrap();
    }
    assert_eq!(execution, r.execution);
    donor.record_intent(r.publication.intent.clone()).unwrap();
    let original = RootProviderPublication {
        occurrence: RootProviderRowAdmission {
            claim: claim.clone(),
            execution: execution.clone(),
            effect_key: r.publication.intent.key.clone(),
            effect_sequence: r.publication.intent.sequence,
            request_digest: r.publication.intent.request_digest,
            identity: ProviderStateIdentity {
                namespace: "publication-fixture-v1".into(),
                key: "manifest".into(),
            },
            observed_tick: 9,
        },
        intent: r.publication.intent.intent.clone(),
        mutations: ["control", "manifest", "catalog", "metadata", "unknown-unit"]
            .into_iter()
            .map(|key| put(key, 1, None, b"exact-owned-bytes"))
            .collect(),
    };
    donor.mutate_root_provider_states(original).unwrap();
    let cutoff = donor
        .capture_root_custody_cutoff(&claim, &execution, 11)
        .unwrap();
    let mut next = admission_for(r, false);
    next.execution.execution_id =
        mainframe_env_execution_api::ExecutionId::new("pg-successor", InvocationLimits::default())
            .unwrap();
    next.execution.run_unit_id = mainframe_env_execution_api::RunUnitId::new(
        "pg-successor-run",
        InvocationLimits::default(),
    )
    .unwrap();
    next.invocation_key =
        IdempotencyKey::new("pg-successor-key", InvocationLimits::default()).unwrap();
    next.event.execution_id = next.execution.execution_id.clone();
    next.event.run_unit_id = next.execution.run_unit_id.clone();
    next.event.tick = 12;
    next.notification = notification_for(&next.event);
    donor
        .admit_root_successor(RootCustodySuccessor {
            cutoff,
            admission: next,
            handoff_rows: ["control", "manifest", "catalog", "metadata"]
                .into_iter()
                .map(|key| ProviderStateIdentity {
                    namespace: "publication-fixture-v1".into(),
                    key: key.into(),
                })
                .collect(),
        })
        .unwrap();
    let pins = donor
        .list_provider_state(crate::root_terminal::successor::PIN_NAMESPACE, 16)
        .unwrap();
    let rows: Vec<_> = donor
        .list_provider_state("publication-fixture-v1", 16)
        .unwrap()
        .into_iter()
        .chain(pins.clone())
        .collect();
    store
        .mutate_provider_states_atomic(
            rows.into_iter()
                .map(|record| {
                    ProviderStateMutation::Put(ProviderStateWrite {
                        record,
                        expected_version: None,
                    })
                })
                .collect(),
        )
        .unwrap();
    pins
}
fn successor_pin_mutations() {
    for fault in 0..9 {
        let fixture = OwnedPg::new();
        let store = fixture.store();
        let r = request(&store);
        let mut pins = successor_pin_fixture(&store, &r);
        if fault >= 5 {
            let key = row_scope_key("publication-fixture-v1", "manifest");
            let pin = pins.iter().find(|row| row.key == key).unwrap();
            let mut value: serde_json::Value = serde_json::from_slice(&pin.payload).unwrap();
            match fault {
                5 => {
                    store
                        .run(
                            sqlx::query(
                                "UPDATE provider_state SET version=2 WHERE namespace=$1 AND key=$2",
                            )
                            .bind(crate::root_terminal::successor::PIN_NAMESPACE)
                            .bind(&key)
                            .execute(&store.pool),
                        )
                        .unwrap();
                }
                6 => {
                    store.run(sqlx::query("UPDATE provider_state SET payload=$1 WHERE namespace=$2 AND key=$3").bind(b"not JSON".as_slice()).bind(crate::root_terminal::successor::PIN_NAMESPACE).bind(&key).execute(&store.pool)).unwrap();
                }
                7 | 8 => {
                    value[if fault == 7 { "key" } else { "owner" }] =
                        serde_json::json!(if fault == 7 { "foreign" } else { "" });
                    store.run(sqlx::query("UPDATE provider_state SET payload=$1 WHERE namespace=$2 AND key=$3").bind(serde_json::to_vec(&value).unwrap()).bind(crate::root_terminal::successor::PIN_NAMESPACE).bind(&key).execute(&store.pool)).unwrap();
                }
                _ => unreachable!(),
            }
            pins = store
                .list_provider_state(crate::root_terminal::successor::PIN_NAMESPACE, 16)
                .unwrap();
        }
        let mut publication = r.publication.clone();
        publication.mutations = match fault {
            0 => vec![put("manifest", 2, Some(1), b"changed")],
            1 => vec![ProviderStateMutation::Delete {
                namespace: "publication-fixture-v1".into(),
                key: "manifest".into(),
                expected_version: 1,
            }],
            2 => vec![ProviderStateMutation::Move {
                record: ProviderStateRecord {
                    namespace: "publication-fixture-v1".into(),
                    key: "moved".into(),
                    version: 2,
                    payload: b"exact-owned-bytes".to_vec(),
                },
                old_key: "manifest".into(),
                expected_version: 1,
            }],
            3 => vec![put("unknown-unit", 2, Some(1), b"exact-owned-bytes")],
            4..=8 => vec![put("manifest", 2, Some(1), b"exact-owned-bytes")],
            _ => unreachable!(),
        };
        let before = raw(&store);
        if fault == 4 {
            store
                .publish_provider_states_audited(publication.clone())
                .unwrap();
            assert_eq!(
                store
                    .get_provider_state("publication-fixture-v1", "manifest")
                    .unwrap()
                    .unwrap()
                    .version,
                2
            );
            assert_eq!(
                store
                    .audit_records(&r.execution.execution_id, 0, 8)
                    .unwrap(),
                vec![publication.audit]
            );
        } else {
            assert!(
                store.publish_provider_states_audited(publication).is_err(),
                "pin fault {fault}"
            );
            assert_eq!(raw(&store), before);
        }
        assert_eq!(
            store
                .list_provider_state(crate::root_terminal::successor::PIN_NAMESPACE, 16)
                .unwrap(),
            pins
        );
        assert_eq!(
            store
                .get_provider_state("publication-fixture-v1", "unknown-unit")
                .unwrap()
                .unwrap()
                .version,
            1
        );
    }
}
