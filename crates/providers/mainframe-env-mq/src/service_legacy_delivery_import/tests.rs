use super::*;
use crate::{
    MqAliasTarget, MqLocalQueueUsage, MqObjectDefinition, MqObjectLimits, MqQueueManagerDefinition,
};
use mainframe_env_execution_api::{
    ArtifactRef, BoundedPayload, ExecutionId, Principal, PrincipalId, RequestId, ResourceLimits,
    RunUnitId, Selector, ServiceClass, TraceId,
};
use mainframe_env_host_api::{MqGetContract, MqGetMode, MqTruncation, MqWait, Mutation};
use mainframe_env_store::{MemoryStore, SqliteStateStore};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;

static NEXT_DB: AtomicU64 = AtomicU64::new(1);
struct Backend {
    store: Option<Arc<dyn ProviderStateStore>>,
    directory: Option<PathBuf>,
}
impl Backend {
    fn new(sqlite: bool) -> Self {
        Self::bounded(sqlite, 256)
    }
    fn bounded(sqlite: bool, max_rows: usize) -> Self {
        if !sqlite {
            return Self {
                store: Some(Arc::new(MemoryStore::new(
                    mainframe_env_store::StoreLimits {
                        max_blob_bytes: 1 << 20,
                        max_provider_state: max_rows,
                        ..Default::default()
                    },
                ))),
                directory: None,
            };
        }
        let directory = std::env::temp_dir().join(format!(
            "mq-legacy-import-{}-{}",
            std::process::id(),
            NEXT_DB.fetch_add(1, Ordering::Relaxed)
        ));
        // Exclusive create: never reopen another fixture's path.
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("state.sqlite");
        let store = SqliteStateStore::open(
            &format!("sqlite://{}?mode=rwc", path.display()),
            1 << 20,
            max_rows,
        )
        .unwrap();
        Self {
            store: Some(Arc::new(store)),
            directory: Some(directory),
        }
    }
    fn store(&self) -> &dyn ProviderStateStore {
        &**self.store.as_ref().unwrap()
    }
    fn arc(&self) -> Arc<dyn ProviderStateStore> {
        self.store.as_ref().unwrap().clone()
    }
    fn reopen(&mut self) {
        if let Some(dir) = &self.directory {
            assert_eq!(
                Arc::strong_count(self.store.as_ref().unwrap()),
                1,
                "close all fixture services before reopening"
            );
            drop(self.store.take());
            self.store = Some(Arc::new(
                SqliteStateStore::open(
                    &format!("sqlite://{}?mode=rw", dir.join("state.sqlite").display()),
                    1 << 20,
                    256,
                )
                .unwrap(),
            ));
        }
    }
}
impl Drop for Backend {
    fn drop(&mut self) {
        drop(self.store.take());
        if let Some(dir) = &self.directory {
            for name in ["state.sqlite", "state.sqlite-wal", "state.sqlite-shm"] {
                let path = dir.join(name);
                if path.exists() {
                    std::fs::remove_file(path).unwrap();
                }
            }
            std::fs::remove_dir(dir).unwrap();
        }
    }
}
fn name(s: &str) -> MqObjectName {
    MqObjectName::new(s).unwrap()
}
fn catalog() -> MqObjectCatalog {
    MqObjectCatalog::new(
        MqQueueManagerDefinition {
            name: name("IMPORTED.QM"),
            default_transmission_queue: None,
        },
        vec![
            MqObjectDefinition::LocalQueue {
                name: name("A"),
                usage: MqLocalQueueUsage::Normal,
                trigger_process: Some(name("P")),
            },
            MqObjectDefinition::LocalQueue {
                name: name("B"),
                usage: MqLocalQueueUsage::Normal,
                trigger_process: None,
            },
            MqObjectDefinition::AliasQueue {
                name: name("ALIAS"),
                target: MqAliasTarget::Queue(name("A")),
            },
            MqObjectDefinition::Process { name: name("P") },
            MqObjectDefinition::Topic { name: name("T") },
        ],
        MqObjectLimits::default(),
    )
    .unwrap()
}
fn raw(namespace: &str, key: &str, version: u64, value: Value) -> ProviderStateRecord {
    ProviderStateRecord { namespace: namespace.into(), key: key.into(), version,
        payload: serde_json::to_vec(&json!({"schema_version":"mainframe-env.mq-object-row@1", "object_key":key,"value":value})).unwrap() }
}
fn known_replay(canonical: bool) -> Value {
    let mut result = json!({"request_sha256": vec![91;32], "completion_code":0,"reason_code":0,"handle":null,"message":[82,69,84],"message_id":null,"correlation_id":null,"trigger_program":null});
    if canonical {
        result["request_digest_format"] = json!("mainframe-env.provider-replay-canonical@1");
    }
    result
}
fn fixture(b: &Backend, populated: bool) -> MqObjectCatalog {
    let c = catalog();
    let mut records = vec![
        ProviderStateRecord { namespace:"mq-state".into(),key:"queues".into(),version:7,payload:br#"{"schema_version":"mainframe-env.mq-row-store@1","definitions":null,"next_handle":9}"#.to_vec() },
        raw("mq-v1-object-catalog","catalog",11,json!(String::from_utf8(c.encode().unwrap()).unwrap())),
        raw("mq-v1-queue","A",13,json!({"trigger_program":"P","messages":if populated { vec![
            json!({"data":[0,255,40],"message_id":vec![1;24],"correlation_id":vec![7;24]}),
            json!({"data":[83,69,67,79,78,68],"message_id":vec![2;24],"correlation_id":vec![8;24]})
        ] } else { vec![] }})),
        raw("mq-v1-queue","B",17,json!({"trigger_program":null,"messages":[]})),
        raw("mq-v1-replay","legacy-receipt",19,known_replay(false)),
        raw("mq-v1-replay","canonical-receipt",23,known_replay(true)),
        raw("mq-v1-handle-index","retired-run",29,json!({})),
    ];
    // Deliberately noncanonical whitespace must be preserved, not rewritten.
    records[5].payload.insert(0, b' ');
    for record in records {
        // Real adapter CAS establishes the independently chosen fixture versions.
        let mut seeded = record.clone();
        seeded.version = 1;
        b.store().put_provider_state(seeded.clone(), None).unwrap();
        for version in 2..=record.version {
            seeded.version = version;
            b.store()
                .put_provider_state(seeded.clone(), Some(version - 1))
                .unwrap();
        }
    }
    c
}
fn all(b: &Backend) -> Vec<ProviderStateRecord> {
    let mut records = b.store().list_provider_state_prefix("mq-", 256).unwrap();
    records.extend(
        b.store()
            .list_provider_state_prefix("owner-proof", 16)
            .unwrap(),
    );
    records.sort_by(|a, b| (&a.namespace, &a.key).cmp(&(&b.namespace, &b.key)));
    records
}
fn service(b: &Backend) -> Arc<MqService> {
    MqService::open(b.arc(), MqLimits::default()).unwrap()
}
fn import(s: &MqService) -> LegacyDeliveryImportPlan {
    s.plan_legacy_delivery_import(3, 5, LegacyImportLimits::default())
        .unwrap()
}
fn assert_plan_error(s: &MqService, limits: LegacyImportLimits) {
    assert!(s.plan_legacy_delivery_import(3, 5, limits).is_err());
}
fn publish(
    b: &Backend,
    plan: LegacyDeliveryImportPlan,
) -> (MqObjectCatalog, MqDeliveryKernel, DeliveryRows) {
    let (batch, c, k, rows) = plan.into_parts();
    b.store().mutate_provider_states_atomic(batch).unwrap();
    (c, k, rows)
}
fn get(k: &mut MqDeliveryKernel, c: &MqObjectCatalog, q: &str) -> MqMessage {
    k.get(
        c,
        &name(q),
        &MqGetContract {
            selection: Default::default(),
            mode: MqGetMode::Remove,
            wait: MqWait::NoWait,
            truncation: MqTruncation::Reject,
            buffer_capacity: 1024,
        },
        None,
    )
    .unwrap()
    .message
    .unwrap()
}
fn invocation() -> Invocation {
    let l = InvocationLimits::default();
    let mut i = Invocation::new(
        RequestId::new("request-import", l).unwrap(),
        ExecutionId::new("execution-import", l).unwrap(),
        RunUnitId::new("run-import", l).unwrap(),
        None,
        Selector::new("mq:test", l).unwrap(),
        ArtifactRef::new("mq:test", l).unwrap(),
        Principal::new(
            PrincipalId::new("IBMUSER", l).unwrap(),
            BTreeSet::from([CapabilityId::new("host.mq.write", l).unwrap()]),
            l,
        )
        .unwrap(),
        ServiceClass::Interactive,
        0,
        100,
        TraceId::new("trace-import", l).unwrap(),
        IdempotencyKey::new("invocation-import", l).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        l,
    )
    .unwrap();
    let (binding, schema) = crate::host_context::host_context_contract();
    i.bindings.insert(
        binding.into(),
        BoundedPayload::new(schema, b"other-bindings|queue-manager".to_vec(), l).unwrap(),
    );
    i
}
fn request(operation: MqOperation, sequence: u64, syncpoint: bool) -> MqRequest {
    MqRequest {
        operation,
        queue: Some("A".into()),
        handle: None,
        options: if syncpoint { 2 } else { 0 },
        message: b"new staged payload".to_vec(),
        message_id: Some(vec![33; 24]),
        correlation_id: Some(vec![34; 24]),
        wait_ticks: 0,
        max_message_bytes: 1024,
        mutation: Some(Mutation {
            sequence,
            idempotency_key: IdempotencyKey::new(
                format!("import-effect-{sequence}"),
                InvocationLimits::default(),
            )
            .unwrap(),
            transaction: Some("legacy-import-test".into()),
        }),
    }
}
fn replace(b: &Backend, ns: &str, key: &str, f: impl FnOnce(&mut ProviderStateRecord)) {
    let mut r = b.store().get_provider_state(ns, key).unwrap().unwrap();
    let v = r.version;
    r.version += 1;
    f(&mut r);
    b.store().put_provider_state(r, Some(v)).unwrap();
}

#[test]
fn memory_sqlite_known_fixture_import_preserves_order_ids_topology_and_replay_on_reopen() {
    for sqlite in [false, true] {
        for populated in [false, true] {
            let mut b = Backend::new(sqlite);
            let c = fixture(&b, populated);
            let before = all(&b);
            let s = service(&b);
            let plan = import(&s);
            assert_eq!(all(&b), before, "planning is read-only");
            assert!(plan.mutations().iter().all(|m| !matches!(m,ProviderStateMutation::Put(w) if w.record.namespace==REPLAY_NAMESPACE)));
            let (_, k, rows) = publish(&b, plan);
            let live = k.encode_live_checkpoint().unwrap();
            let cold = k.encode().unwrap();
            assert_eq!(k.depth(&name("A")), Some(if populated { 2 } else { 0 }));
            assert_eq!(k.depth(&name("B")), Some(0));
            let retained_catalog = b
                .store()
                .get_provider_state(CATALOG_NAMESPACE, CATALOG_KEY)
                .unwrap()
                .unwrap();
            let old_catalog = before
                .iter()
                .find(|r| r.namespace == CATALOG_NAMESPACE)
                .unwrap();
            assert_eq!(retained_catalog.payload, old_catalog.payload);
            assert_eq!(retained_catalog.version, old_catalog.version + 1);
            for old in before.iter().filter(|r| r.namespace == REPLAY_NAMESPACE) {
                assert_eq!(
                    b.store()
                        .get_provider_state(&old.namespace, &old.key)
                        .unwrap(),
                    Some(old.clone())
                );
            }
            assert!(
                b.store()
                    .list_provider_state(QUEUE_NAMESPACE, 4)
                    .unwrap()
                    .is_empty()
            );
            assert!(
                b.store()
                    .list_provider_state(HANDLE_NAMESPACE, 4)
                    .unwrap()
                    .is_empty()
            );
            let marker = b
                .store()
                .get_provider_state(STATE_NAMESPACE, STATE_KEY)
                .unwrap()
                .unwrap();
            assert_eq!(marker.version, 8);
            let marker: Value = serde_json::from_slice(&marker.payload).unwrap();
            assert_eq!(marker["schema_version"], RICH_MARKER_SCHEMA);
            assert_eq!(marker["legacy_next_handle"], 9);
            assert!(matches!(
                MqService::open(b.arc(), Default::default()),
                Err(HostProblem::InfrastructureFailure)
            ));
            drop(s);
            b.reopen();
            let (loaded, mut restored) = DeliveryRows::load(
                b.store(),
                &c,
                DeliveryRowIdentity::new(&c, 3, 5).unwrap(),
                Default::default(),
                Default::default(),
                Default::default(),
                MqPersistence::Persistent,
            )
            .unwrap();
            assert_eq!(restored.encode_live_checkpoint().unwrap(), live);
            assert_eq!(restored.encode().unwrap(), cold);
            assert_eq!(
                loaded
                    .delta(&restored, &c, DeliveryRowIdentity::new(&c, 3, 5).unwrap())
                    .unwrap()
                    .mutations()
                    .len(),
                1
            );
            assert_eq!(
                rows.delta(&restored, &c, DeliveryRowIdentity::new(&c, 3, 5).unwrap())
                    .unwrap()
                    .mutations()
                    .len(),
                1
            );
            let mut cold_restored = MqDeliveryKernel::decode(
                &cold,
                &c,
                Default::default(),
                Default::default(),
                MqPersistence::Persistent,
            )
            .unwrap();
            if populated {
                for (id, corr, body) in [(1, 7, vec![0, 255, 40]), (2, 8, b"SECOND".to_vec())] {
                    let message = get(&mut restored, &c, "ALIAS");
                    let cold_message = get(&mut cold_restored, &c, "A");
                    assert_eq!(message, cold_message);
                    assert_eq!(message.body, body);
                    assert_eq!(
                        message.descriptor.identifiers.message_id,
                        Some(vec![id; 24])
                    );
                    assert_eq!(
                        message.descriptor.identifiers.correlation_id,
                        Some(vec![corr; 24])
                    );
                    assert_eq!(message.descriptor.persistence, MqPersistence::Persistent);
                    assert_eq!(message.descriptor.expiry, MqExpiry::Unlimited);
                    assert_eq!(message.descriptor.format, None);
                    assert!(message.properties.is_empty());
                    assert_eq!(message.descriptor.ordering, MqMessageOrdering::default());
                    assert_eq!(message.descriptor.identifiers.group_id, None);
                }
            }
            assert_eq!(restored.depth(&name("A")), Some(0));
        }
    }
}

#[test]
fn memory_sqlite_pending_insert_wins_and_migration_rolls_back_all_rich_rows() {
    for sqlite in [false, true] {
        let b = Backend::new(sqlite);
        fixture(&b, true);
        let s = service(&b);
        let plan = import(&s);
        let queued = b.store().get_provider_state(QUEUE_NAMESPACE, "A").unwrap();
        s.execute(&invocation(), &request(MqOperation::PutOne, 1, true))
            .unwrap();
        assert_eq!(
            b.store().get_provider_state(QUEUE_NAMESPACE, "A").unwrap(),
            queued,
            "pending insert touches no queue row"
        );
        let after = all(&b);
        assert_eq!(
            b.store()
                .get_provider_state(STATE_NAMESPACE, STATE_KEY)
                .unwrap()
                .unwrap()
                .version,
            8
        );
        assert!(
            b.store()
                .get_provider_state(PENDING_NAMESPACE, "run-import")
                .unwrap()
                .is_some()
        );
        assert_eq!(
            b.store().mutate_provider_states_atomic(plan.into_parts().0),
            Err(StoreError::Conflict)
        );
        assert_eq!(all(&b), after);
        assert!(
            b.store()
                .list_provider_state_prefix(crate::delivery::checkpoint::rows::PREFIX, 8)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            s.plan_legacy_delivery_import(3, 5, Default::default())
                .err(),
            Some(LegacyImportError::NonQuiescent)
        );
    }
}

#[test]
fn memory_sqlite_migration_wins_and_already_open_legacy_writer_cannot_publish_any_family() {
    for sqlite in [false, true] {
        let b = Backend::new(sqlite);
        fixture(&b, true);
        let s = service(&b);
        publish(&b, import(&s));
        let after = all(&b);
        for (n, op, syncpoint) in [
            (1, MqOperation::PutOne, true),
            (2, MqOperation::PutOne, false),
            (3, MqOperation::Open, false),
            (4, MqOperation::Get, false),
            (5, MqOperation::Commit, false),
            (6, MqOperation::Rollback, false),
        ] {
            assert_eq!(
                s.execute(&invocation(), &request(op, n, syncpoint)),
                Err(HostProblem::IdempotencyConflict)
            );
            assert_eq!(all(&b), after);
        }
        assert_eq!(
            s.queue_messages("A").unwrap(),
            vec![vec![0, 255, 40], b"SECOND".to_vec()],
            "failed publications do not adopt candidates"
        );
        let req = request(MqOperation::PutOne, 7, false);
        let mut req = req;
        req.mutation.as_mut().unwrap().idempotency_key =
            IdempotencyKey::new("legacy-receipt", Default::default()).unwrap();
        assert_eq!(
            s.reconcile_legacy_replay(
                &req.mutation.as_ref().unwrap().idempotency_key,
                [91; 32],
                &req
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(all(&b), after);
    }
}

#[test]
fn memory_sqlite_live_handles_and_even_empty_pending_units_reject_unchanged() {
    for sqlite in [false, true] {
        for kind in 0..4 {
            let b = Backend::new(sqlite);
            fixture(&b, true);
            let row = match kind {
                0 => raw(HANDLE_NAMESPACE, "live", 1, json!({"4":"A"})),
                1 => raw(
                    PENDING_NAMESPACE,
                    "empty-live",
                    1,
                    json!({"puts":[],"gets":[]}),
                ),
                2 => raw(
                    PENDING_NAMESPACE,
                    "staged",
                    1,
                    json!({"puts":[["A",{"data":[77],"message_id":vec![6;24],"correlation_id":vec![0;24]}]],"gets":[]}),
                ),
                _ => raw(
                    PENDING_NAMESPACE,
                    "removed",
                    1,
                    json!({"puts":[],"gets":[["A",{"data":[77],"message_id":vec![6;24],"correlation_id":vec![0;24]}]]}),
                ),
            };
            b.store().put_provider_state(row, None).unwrap();
            let s = service(&b);
            let before = all(&b);
            assert_eq!(
                s.plan_legacy_delivery_import(3, 5, Default::default())
                    .err(),
                Some(LegacyImportError::NonQuiescent)
            );
            assert_eq!(all(&b), before);
        }
    }
}

#[test]
fn memory_sqlite_stale_queue_catalog_manifest_and_late_composed_failure_are_atomic() {
    for sqlite in [false, true] {
        for ns in [QUEUE_NAMESPACE, CATALOG_NAMESPACE, STATE_NAMESPACE, "late"] {
            let b = Backend::new(sqlite);
            fixture(&b, true);
            let s = service(&b);
            let mut batch = import(&s).into_parts().0;
            if ns == "late" {
                b.store()
                    .put_provider_state(
                        ProviderStateRecord {
                            namespace: "owner-proof".into(),
                            key: "audit-composition".into(),
                            version: 1,
                            payload: b"unchanged".to_vec(),
                        },
                        None,
                    )
                    .unwrap();
                batch.push(ProviderStateMutation::Put(ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: "owner-proof".into(),
                        key: "audit-composition".into(),
                        version: 3,
                        payload: b"would change".to_vec(),
                    },
                    expected_version: Some(2),
                }));
            } else {
                replace(
                    &b,
                    ns,
                    if ns == QUEUE_NAMESPACE {
                        "A"
                    } else if ns == CATALOG_NAMESPACE {
                        CATALOG_KEY
                    } else {
                        STATE_KEY
                    },
                    |_| {},
                );
            }
            let before = all(&b);
            assert_eq!(
                b.store().mutate_provider_states_atomic(batch),
                Err(StoreError::Conflict)
            );
            assert_eq!(all(&b), before);
            assert!(
                b.store()
                    .list_provider_state_prefix(crate::delivery::checkpoint::rows::PREFIX, 8)
                    .unwrap()
                    .is_empty()
            );
        }
    }
}

#[test]
fn memory_sqlite_source_corruption_orphan_and_stale_scan_reject_without_publication() {
    for sqlite in [false, true] {
        for case in 0..9 {
            let b = Backend::new(sqlite);
            fixture(&b, true);
            let s = service(&b);
            match case {
                0 => {
                    b.store()
                        .put_provider_state(
                            ProviderStateRecord {
                                namespace: "mq-delivery-live-v1-queue".into(),
                                key: "orphan".into(),
                                version: 1,
                                payload: b"bad".to_vec(),
                            },
                            None,
                        )
                        .unwrap();
                }
                1 => replace(&b, QUEUE_NAMESPACE, "A", |r| {
                    r.payload = b"malformed".to_vec()
                }),
                2 => replace(&b, QUEUE_NAMESPACE, "A", |r| {
                    let mut v: Value = serde_json::from_slice(&r.payload).unwrap();
                    v["object_key"] = json!("B");
                    r.payload = serde_json::to_vec(&v).unwrap();
                }),
                3 => {
                    b.store()
                        .delete_provider_state(QUEUE_NAMESPACE, "A", 13)
                        .unwrap();
                }
                4 => replace(&b, CATALOG_NAMESPACE, CATALOG_KEY, |r| {
                    r.payload = b"malformed".to_vec()
                }),
                5 => replace(&b, STATE_NAMESPACE, STATE_KEY, |r| {
                    r.payload = b"malformed".to_vec()
                }),
                6 => {
                    b.store()
                        .put_provider_state(
                            raw(
                                PENDING_NAMESPACE,
                                "new-empty",
                                1,
                                json!({"puts":[],"gets":[]}),
                            ),
                            None,
                        )
                        .unwrap();
                }
                7 => {
                    b.store()
                        .put_provider_state(
                            raw(
                                QUEUE_NAMESPACE,
                                "unexpected",
                                1,
                                json!({"trigger_program":null,"messages":[]}),
                            ),
                            None,
                        )
                        .unwrap();
                }
                _ => {
                    s.lock()
                        .unwrap()
                        .versions
                        .remove(&(QUEUE_NAMESPACE.into(), "A".into()));
                }
            }
            let before = all(&b);
            assert_plan_error(&s, Default::default());
            assert_eq!(all(&b), before);
        }
    }
}

#[test]
fn memory_sqlite_strict_source_fields_and_catalog_trigger_mismatch_fail_closed() {
    for sqlite in [false, true] {
        for case in 0..7 {
            let b = Backend::new(sqlite);
            fixture(&b, true);
            replace(&b, QUEUE_NAMESPACE, "A", |r| {
                let mut v: Value = serde_json::from_slice(&r.payload).unwrap();
                match case {
                    0 => v["value"]["unknown"] = json!(1),
                    1 => v["value"]["messages"][0]["expiry"] = json!(5),
                    2 => v["value"]["messages"][0]["message_id"] = json!([1]),
                    3 => v["value"]["trigger_program"] = json!("OTHER"),
                    4 => {
                        v["value"]["messages"][0]
                            .as_object_mut()
                            .unwrap()
                            .remove("data");
                    }
                    5 => v["schema_version"] = json!("unknown"),
                    _ => {
                        v["value"]
                            .as_object_mut()
                            .unwrap()
                            .remove("trigger_program");
                    }
                };
                r.payload = serde_json::to_vec(&v).unwrap();
            });
            let before = all(&b);
            match MqService::open(b.arc(), Default::default()) {
                Ok(s) => assert_plan_error(&s, Default::default()),
                Err(_) => {}
            }
            assert_eq!(all(&b), before);
        }
    }
}

#[test]
fn memory_sqlite_narrow_target_and_source_limits_generation_and_overflow_reject_unchanged() {
    for sqlite in [false, true] {
        let b = Backend::new(sqlite);
        fixture(&b, true);
        let s = service(&b);
        let before = all(&b);
        for case in 0..8 {
            let mut l = LegacyImportLimits::default();
            match case {
                0 => l.delivery.depth_per_queue = 1,
                1 => l.delivery.queues = 1,
                2 => l.delivery.total_bytes = 1,
                3 => l.delivery.snapshot_bytes = 128,
                4 => l.message.body_bytes = 1,
                5 => l.rows.rows = 1,
                6 => l.rows.mutations = 3,
                _ => l.rows.total_bytes = 1,
            };
            assert_plan_error(&s, l);
            assert_eq!(all(&b), before);
        }
        for (generation, fence) in [(0, 5), (3, 0), (u64::MAX, 5), (3, u64::MAX)] {
            assert!(
                s.plan_legacy_delivery_import(generation, fence, Default::default())
                    .is_err()
            );
            assert_eq!(all(&b), before);
        }
        for source_limits in [
            MqLimits {
                max_state_bytes: 512,
                ..Default::default()
            },
            MqLimits {
                max_state_bytes: 0,
                ..Default::default()
            },
            MqLimits {
                max_replays: usize::MAX,
                ..Default::default()
            },
        ] {
            assert!(
                plan(
                    b.store(),
                    &s.lock().unwrap(),
                    source_limits,
                    3,
                    5,
                    Default::default()
                )
                .is_err()
            );
            assert_eq!(all(&b), before);
        }
        assert_eq!(
            dependency_put(
                &ProviderStateRecord {
                    namespace: STATE_NAMESPACE.into(),
                    key: STATE_KEY.into(),
                    version: i64::MAX as u64,
                    payload: Vec::new(),
                },
                Vec::new()
            )
            .err(),
            Some(LegacyImportError::Bounds)
        );
        assert_eq!(all(&b), before);
    }
}

#[test]
fn memory_sqlite_capacity_and_late_oversized_composition_roll_back_import() {
    for sqlite in [false, true] {
        for capacity in [true, false] {
            let b = Backend::bounded(sqlite, if capacity { 7 } else { 256 });
            fixture(&b, true);
            let s = service(&b);
            let mut batch = import(&s).into_parts().0;
            // The import replaces three retired rows with three rich rows.
            // One composed owner row makes final capacity exceed the quota.
            batch.push(ProviderStateMutation::Put(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "owner-proof".into(),
                    key: "composed".into(),
                    version: 1,
                    payload: vec![0; if capacity { 1 } else { (1 << 20) + 1 }],
                },
                expected_version: None,
            }));
            let before = all(&b);
            assert_eq!(
                b.store().mutate_provider_states_atomic(batch),
                Err(if capacity {
                    StoreError::CapacityExceeded
                } else {
                    StoreError::PayloadTooLarge
                })
            );
            assert_eq!(all(&b), before);
        }
    }
}

#[test]
fn memory_sqlite_disjoint_stale_legacy_writer_now_conflicts_without_retry() {
    for sqlite in [false, true] {
        let b = Backend::new(sqlite);
        fixture(&b, false);
        let first = service(&b);
        let stale = service(&b);
        first
            .execute(&invocation(), &request(MqOperation::PutOne, 30, false))
            .unwrap();
        let before = all(&b);
        let mut second = request(MqOperation::PutOne, 31, false);
        second.queue = Some("B".into());
        assert_eq!(
            stale.execute(&invocation(), &second),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(all(&b), before);
        assert_eq!(stale.queue_depth("B"), Ok(0));
        assert_eq!(
            stale.execute(&invocation(), &second),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(all(&b), before);
    }
}

#[test]
fn memory_sqlite_protected_canonical_replay_dependency_retains_exact_metadata_bytes() {
    for sqlite in [false, true] {
        let b = Backend::new(sqlite);
        fixture(&b, false);
        let s = service(&b);
        s.execute(&invocation(), &request(MqOperation::Commit, 10, false))
            .unwrap();
        let before = b
            .store()
            .get_provider_state(REPLAY_NAMESPACE, "import-effect-10")
            .unwrap()
            .unwrap();
        let descriptor = crate::describe_mq_replay_row(&before, None, Default::default()).unwrap();
        assert_eq!(
            descriptor.retention,
            crate::MqReplayRetentionState::PendingProtected
        );
        assert_eq!(
            descriptor.owner_execution.as_deref(),
            Some("execution-import")
        );
        assert_eq!(descriptor.owner_run_unit.as_deref(), Some("run-import"));
        assert_eq!(
            descriptor.dependency,
            Some(crate::MqReplayDependency::CoreEffect)
        );
        publish(&b, import(&s));
        let after = b
            .store()
            .get_provider_state(REPLAY_NAMESPACE, "import-effect-10")
            .unwrap()
            .unwrap();
        assert_eq!(after, before);
        assert_eq!(
            crate::describe_mq_replay_row(&after, None, Default::default()).unwrap(),
            descriptor
        );
    }
}
