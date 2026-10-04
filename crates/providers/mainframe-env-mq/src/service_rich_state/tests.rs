use super::*;
use crate::{
    MqAliasTarget, MqLocalQueueUsage, MqObjectDefinition, MqObjectLimits, MqObjectName,
    MqQueueManagerDefinition,
};
use mainframe_env_execution_api::{
    ArtifactRef, ExecutionId, Principal, PrincipalId, RequestId, ResourceLimits, RunUnitId,
    Selector, ServiceClass, TraceId,
};
use mainframe_env_host_api::{MqGetContract, MqGetMode, MqTruncation, MqWait};
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
        if !sqlite {
            return Self {
                store: Some(Arc::new(MemoryStore::new(Default::default()))),
                directory: None,
            };
        }
        let directory = std::env::temp_dir().join(format!(
            "mq-rich-reader-{}-{}",
            std::process::id(),
            NEXT_DB.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let directory = directory.canonicalize().unwrap();
        let store = SqliteStateStore::open(
            &format!(
                "sqlite://{}?mode=rwc",
                directory.join("state.sqlite").display()
            ),
            64 << 20,
            256,
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
            assert_eq!(Arc::strong_count(self.store.as_ref().unwrap()), 1);
            drop(self.store.take());
            self.store = Some(Arc::new(
                SqliteStateStore::open(
                    &format!("sqlite://{}?mode=rw", dir.join("state.sqlite").display()),
                    64 << 20,
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
            name: name("READ.QM"),
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
fn object(ns: &str, key: &str, value: Value) -> ProviderStateRecord {
    ProviderStateRecord {
        namespace: ns.into(),
        key: key.into(),
        version: 1,
        payload: serde_json::to_vec(
            &json!({"schema_version":OBJECT_ROW_SCHEMA,"object_key":key,"value":value}),
        )
        .unwrap(),
    }
}
fn fixture(b: &Backend, populated: bool) -> MqObjectCatalog {
    let c = catalog();
    let mut records = vec![
        ProviderStateRecord { namespace:STATE_NAMESPACE.into(),key:STATE_KEY.into(),version:1,
            payload:br#"{"schema_version":"mainframe-env.mq-row-store@1","definitions":null,"next_handle":9}"#.to_vec() },
        object(CATALOG_NAMESPACE,CATALOG_KEY,json!(String::from_utf8(c.encode().unwrap()).unwrap())),
        object(QUEUE_NAMESPACE,"A",json!({"trigger_program":"P","messages":if populated { vec![
            json!({"data":[0,255,41],"message_id":vec![1;24],"correlation_id":vec![7;24]}),
            json!({"data":[83,69,67,79,78,68],"message_id":vec![2;24],"correlation_id":vec![8;24]})
        ] } else {vec![]}})),
        object(QUEUE_NAMESPACE,"B",json!({"trigger_program":null,"messages":[]})),
        object(REPLAY_NAMESPACE,"old-open",json!({"request_sha256":vec![91;32],"completion_code":0,"reason_code":0,
            "handle":7,"message":[],"message_id":null,"correlation_id":null,"trigger_program":null})),
        object(REPLAY_NAMESPACE,"canonical-read",json!({"request_digest_format":"mainframe-env.provider-replay-canonical@1",
            "request_sha256":vec![92;32],"completion_code":0,"reason_code":0,"handle":null,"message":[82,69,84],
            "message_id":vec![3;24],"correlation_id":vec![9;24],"trigger_program":null})),
    ];
    records[5].payload.insert(0, b' ');
    for r in records {
        b.store().put_provider_state(r.clone(), None).unwrap();
        let mut next = r;
        next.version = 2;
        b.store().put_provider_state(next, Some(1)).unwrap();
    }
    c
}
fn capture(b: &Backend) -> Vec<ProviderStateRecord> {
    b.store().list_provider_state_prefix("mq-", 256).unwrap()
}
fn load(b: &Backend) -> StoredAuthority {
    read(b.store(), 3, 5, ReaderLimits::default()).unwrap()
}
fn rich(b: &Backend) -> RichStoredState {
    match load(b) {
        StoredAuthority::Rich(r) => r,
        _ => panic!("expected rich"),
    }
}
fn import(b: &Backend) {
    let service = MqService::open(b.arc(), MqLimits::default()).unwrap();
    let before = service.lock().unwrap().state.clone();
    let plan = service
        .plan_legacy_delivery_import(3, 5, legacy_delivery_import::LegacyImportLimits::default())
        .unwrap();
    let (batch, _, _, _) = plan.into_parts();
    b.store().mutate_provider_states_atomic(batch).unwrap();
    assert_eq!(service.lock().unwrap().state, before);
}
fn get(k: &mut MqDeliveryKernel, c: &MqObjectCatalog) -> mainframe_env_host_api::MqMessage {
    k.get(
        c,
        &name("A"),
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
fn edit(records: &mut [ProviderStateRecord], ns: &str, key: &str, f: impl FnOnce(&mut Value)) {
    let r = records
        .iter_mut()
        .find(|r| r.namespace == ns && r.key == key)
        .unwrap();
    let mut v: Value = serde_json::from_slice(&r.payload).unwrap();
    f(&mut v);
    r.payload = serde_json::to_vec(&v).unwrap();
}
fn reject(records: Vec<ProviderStateRecord>) {
    assert!(decode_records(records, 3, 5, ReaderLimits::default()).is_err());
}

#[test]
fn memory_sqlite_initialized_and_imported_authority_roundtrip_reopen_no_writes() {
    for sqlite in [false, true] {
        for populated in [false, true] {
            let mut b = Backend::new(sqlite);
            let c = fixture(&b, populated);
            let before = capture(&b);
            let old_service = MqService::open(b.arc(), MqLimits::default()).unwrap();
            let old_state = old_service.lock().unwrap().state.clone();
            let StoredAuthority::Legacy(legacy) = load(&b) else {
                panic!("legacy expected")
            };
            assert_eq!(legacy.state, old_state);
            assert_eq!(legacy.versions, old_service.lock().unwrap().versions);
            assert_eq!(
                legacy.state.queues["A"].messages.len(),
                if populated { 2 } else { 0 }
            );
            assert_eq!(
                legacy.state.queues["A"].trigger_program.as_deref(),
                Some("P")
            );
            assert_eq!(capture(&b), before);
            assert_eq!(old_service.lock().unwrap().state, old_state);
            drop(old_service);
            import(&b);
            let committed = capture(&b);
            let mut r = rich(&b);
            assert_eq!(r.catalog.encode().unwrap(), c.encode().unwrap());
            assert_eq!(r.marker.legacy_next_handle, 9);
            assert_eq!(r.marker.source_manifest_version, 2);
            assert_eq!(r.marker.source_catalog_version, 2);
            assert_eq!(r.replay["old-open"].handle, Some(7));
            assert_eq!(
                r.replay["canonical-read"].request_digest_format,
                ReplayDigestFormat::CanonicalHostV1
            );
            for old in before.iter().filter(|r| r.namespace == REPLAY_NAMESPACE) {
                assert!(r.retained_records.contains(old));
                assert_eq!(
                    r.versions[&(old.namespace.clone(), old.key.clone())],
                    old.version
                );
            }
            let checkpoint = r.delivery.encode_live_checkpoint().unwrap();
            let cold = r.delivery.encode().unwrap();
            if populated {
                for (data, id, corr) in [(vec![0, 255, 41], 1, 7), (b"SECOND".to_vec(), 2, 8)] {
                    let m = get(&mut r.delivery, &c);
                    assert_eq!(m.body, data);
                    assert_eq!(m.descriptor.identifiers.message_id, Some(vec![id; 24]));
                    assert_eq!(
                        m.descriptor.identifiers.correlation_id,
                        Some(vec![corr; 24])
                    );
                    assert_eq!(m.descriptor.persistence, MqPersistence::Persistent);
                    assert!(m.properties.is_empty());
                }
            }
            assert!(MqService::open(b.arc(), MqLimits::default()).is_err());
            assert_eq!(
                capture(&b),
                committed,
                "all reading/old-open attempts are read-only"
            );
            drop(r);
            b.reopen();
            let r = rich(&b);
            assert_eq!(r.delivery.encode_live_checkpoint().unwrap(), checkpoint);
            assert_eq!(r.delivery.encode().unwrap(), cold);
            let restarted = MqDeliveryKernel::decode(
                &cold,
                &c,
                Default::default(),
                Default::default(),
                MqPersistence::Persistent,
            )
            .unwrap();
            assert_eq!(
                restarted.depth(&name("A")),
                Some(if populated { 2 } else { 0 })
            );
            assert_eq!(capture(&b), committed);
        }
    }
}

#[test]
fn actual_public_initialization_reads_without_automatic_flat_or_missing_migration() {
    for sqlite in [false, true] {
        let b = Backend::new(sqlite);
        assert_eq!(
            read(b.store(), 3, 5, Default::default()).err(),
            Some(ReadError::Missing)
        );
        assert!(capture(&b).is_empty());
        let s = MqService::open(b.arc(), MqLimits::default()).unwrap();
        s.install_object_catalog(catalog()).unwrap();
        let before = capture(&b);
        let StoredAuthority::Legacy(d) = load(&b) else {
            panic!("legacy expected")
        };
        assert_eq!(d.state, s.lock().unwrap().state);
        assert_eq!(capture(&b), before);
        let flat = ProviderStateRecord {
            namespace: STATE_NAMESPACE.into(),
            key: STATE_KEY.into(),
            version: 1,
            payload: serde_json::to_vec(&State {
                next_handle: 1,
                ..State::default()
            })
            .unwrap(),
        };
        reject(vec![flat]);
    }
}

#[test]
fn strict_v1_rejects_missing_duplicate_unknown_schema_namespace_and_crossrefs() {
    let b = Backend::new(false);
    fixture(&b, true);
    let original = capture(&b);
    let s = MqService::open(b.arc(), Default::default()).unwrap();
    let state = s.lock().unwrap().state.clone();
    for ns in [STATE_NAMESPACE, CATALOG_NAMESPACE, QUEUE_NAMESPACE] {
        let mut bad = original.clone();
        bad.retain(|r| r.namespace != ns || ns == QUEUE_NAMESPACE && r.key != "A");
        reject(bad);
    }
    for r in &original {
        let mut bad = original.clone();
        bad.push(r.clone());
        reject(bad);
    }
    let cases: Vec<(&str, &str, Box<dyn Fn(&mut Value)>)> = vec![
        (
            STATE_NAMESPACE,
            STATE_KEY,
            Box::new(|v| {
                v["unknown"] = json!(1);
            }),
        ),
        (
            STATE_NAMESPACE,
            STATE_KEY,
            Box::new(|v| {
                v["schema_version"] = json!("future");
            }),
        ),
        (
            STATE_NAMESPACE,
            STATE_KEY,
            Box::new(|v| {
                v.as_object_mut().unwrap().remove("definitions");
            }),
        ),
        (
            STATE_NAMESPACE,
            STATE_KEY,
            Box::new(|v| {
                v["next_handle"] = json!(0);
            }),
        ),
        (
            QUEUE_NAMESPACE,
            "A",
            Box::new(|v| {
                v["object_key"] = json!("B");
            }),
        ),
        (
            QUEUE_NAMESPACE,
            "A",
            Box::new(|v| {
                v["schema_version"] = json!("future");
            }),
        ),
        (
            QUEUE_NAMESPACE,
            "A",
            Box::new(|v| {
                v["unknown"] = json!(true);
            }),
        ),
        (
            QUEUE_NAMESPACE,
            "A",
            Box::new(|v| {
                v["value"]["unknown"] = json!(true);
            }),
        ),
        (
            QUEUE_NAMESPACE,
            "A",
            Box::new(|v| {
                v["value"]
                    .as_object_mut()
                    .unwrap()
                    .remove("trigger_program");
            }),
        ),
        (
            QUEUE_NAMESPACE,
            "A",
            Box::new(|v| {
                v["value"]["trigger_program"] = json!("T");
            }),
        ),
        (
            QUEUE_NAMESPACE,
            "A",
            Box::new(|v| {
                v["value"]["messages"][0]["unknown"] = json!(0);
            }),
        ),
        (
            QUEUE_NAMESPACE,
            "A",
            Box::new(|v| {
                v["value"]["messages"][0]["message_id"] = json!([1]);
            }),
        ),
        (
            REPLAY_NAMESPACE,
            "old-open",
            Box::new(|v| {
                v["value"]["owner_execution"] = json!("fabricated-owner");
            }),
        ),
    ];
    for (ns, key, f) in cases {
        let mut bad = original.clone();
        edit(&mut bad, ns, key, f);
        reject(bad);
    }
    for ns in ["mq-unknown", "mq-v2-queue", "mq-delivery-live-v1-queue"] {
        let mut bad = original.clone();
        bad.push(object(ns, "X", json!({})));
        reject(bad);
    }
    for version in [0, i64::MAX as u64 + 1] {
        let mut bad = original.clone();
        bad[0].version = version;
        reject(bad);
    }
    for payload in [b"{".to_vec(), b"{} {}".to_vec()] {
        let mut bad = original.clone();
        bad[0].payload = payload;
        reject(bad);
    }
    // Duplicate JSON fields, including numeric map keys, cannot be collapsed.
    let mut bad = original.clone();
    bad.push(object(HANDLE_NAMESPACE, "run", json!({"1":"A"})));
    let row = bad.last_mut().unwrap();
    row.payload=br#"{"schema_version":"mainframe-env.mq-object-row@1","object_key":"run","value":{"1":"A","1":"B"}}"#.to_vec();
    reject(bad);
    let mut bad = original.clone();
    bad[0].payload=br#"{"schema_version":"mainframe-env.mq-row-store@1","schema_version":"mainframe-env.mq-row-store@1","definitions":null,"next_handle":9}"#.to_vec();
    reject(bad);
    assert_eq!(capture(&b), original);
    assert_eq!(s.lock().unwrap().state, state);
}

#[test]
fn v1_live_legacy_handle_and_pending_rows_remain_in_legacy_authority_only() {
    let b = Backend::new(false);
    fixture(&b, false);
    b.store()
        .put_provider_state(object(HANDLE_NAMESPACE, "run", json!({"7":"A"})), None)
        .unwrap();
    b.store().put_provider_state(object(PENDING_NAMESPACE,"run",json!({"puts":[["A",{"data":[1],"message_id":vec![4;24],"correlation_id":vec![5;24]}]],"gets":[]})),None).unwrap();
    let before = capture(&b);
    let StoredAuthority::Legacy(d) = load(&b) else {
        panic!("legacy expected")
    };
    assert_eq!(d.state.handles["run"][&7], "A");
    assert_eq!(d.state.pending["run"].puts[0].1.data, vec![1]);
    for (ns, field) in [(HANDLE_NAMESPACE, "unused"), (PENDING_NAMESPACE, "unknown")] {
        let mut bad = before.clone();
        edit(&mut bad, ns, "run", |v| {
            v["value"][field] = json!(1);
        });
        reject(bad);
    }
    let mut bad = before.clone();
    edit(&mut bad, PENDING_NAMESPACE, "run", |v| {
        v["value"]["puts"][0][0] = json!("MISSING");
    });
    reject(bad);
    let mut bad = before.clone();
    edit(&mut bad, HANDLE_NAMESPACE, "run", |v| {
        v["value"] = json!({"07":"A"});
    });
    reject(bad);
    assert_eq!(capture(&b), before);
}

#[test]
fn strict_v2_marker_identity_provenance_and_mixed_or_orphan_rows_fail_closed() {
    for sqlite in [false, true] {
        let b = Backend::new(sqlite);
        fixture(&b, true);
        import(&b);
        let original = capture(&b);
        for field in [
            "schema_version",
            "target_row_prefix",
            "identity",
            "source_manifest_version",
            "source_catalog_version",
            "legacy_next_handle",
        ] {
            let mut bad = original.clone();
            edit(&mut bad, STATE_NAMESPACE, STATE_KEY, |v| {
                v.as_object_mut().unwrap().remove(field);
            });
            reject(bad);
        }
        for (field, value) in [
            ("unknown", json!(0)),
            ("target_row_prefix", json!("mq-v1-")),
            ("source_manifest_version", json!(0)),
            ("source_manifest_version", json!(3)),
            ("source_catalog_version", json!(i64::MAX as u64)),
            ("source_catalog_version", json!(4)),
            ("legacy_next_handle", json!(0)),
        ] {
            let mut bad = original.clone();
            edit(&mut bad, STATE_NAMESPACE, STATE_KEY, |v| {
                v[field] = value;
            });
            reject(bad);
        }
        for (field, value) in [
            ("generation", json!(4)),
            ("fence", json!(6)),
            ("catalog_sha256", json!(vec![0; 32])),
            ("unknown", json!(0)),
        ] {
            let mut bad = original.clone();
            edit(&mut bad, STATE_NAMESPACE, STATE_KEY, |v| {
                v["identity"][field] = value;
            });
            reject(bad);
        }
        for ns in [
            QUEUE_NAMESPACE,
            HANDLE_NAMESPACE,
            PENDING_NAMESPACE,
            "mq-delivery-live-v1-unknown",
        ] {
            let mut bad = original.clone();
            bad.push(object(ns, "leftover", json!({})));
            reject(bad);
        }
        for ns in [
            CATALOG_NAMESPACE,
            "mq-delivery-live-v1-meta",
            "mq-delivery-live-v1-queue",
        ] {
            let mut bad = original.clone();
            bad.retain(|r| r.namespace != ns);
            reject(bad);
        }
        for r in &original {
            let mut bad = original.clone();
            bad.push(r.clone());
            reject(bad);
        }
        for (g, f) in [(0, 5), (3, 0), (4, 5), (3, 6), (u64::MAX, 5)] {
            assert!(read(b.store(), g, f, Default::default()).is_err());
        }
        // Captured metadata and marker must agree, independently of provenance.
        let mut bad = original.clone();
        edit(&mut bad, "mq-delivery-live-v1-meta", "state", |v| {
            v["value"]["identity"]["fence"] = json!(6);
        });
        reject(bad);
        // Historical lower bounds permit later physical dependency versions.
        let mut later = original.clone();
        for r in &mut later {
            if r.namespace == STATE_NAMESPACE || r.namespace == CATALOG_NAMESPACE {
                r.version += 10;
            }
        }
        assert!(matches!(
            decode_records(later, 3, 5, Default::default()),
            Ok(StoredAuthority::Rich(_))
        ));
        assert!(MqService::open(b.arc(), Default::default()).is_err());
        assert_eq!(capture(&b), original);
    }
}

#[test]
fn physical_corruption_rejection_does_not_write_or_mutate_an_open_legacy_state() {
    for sqlite in [false, true] {
        let b = Backend::new(sqlite);
        fixture(&b, true);
        let s = MqService::open(b.arc(), Default::default()).unwrap();
        let old_state = s.lock().unwrap().state.clone();
        import(&b);
        let mut marker = b
            .store()
            .get_provider_state(STATE_NAMESPACE, STATE_KEY)
            .unwrap()
            .unwrap();
        let mut v: Value = serde_json::from_slice(&marker.payload).unwrap();
        v["identity"]["fence"] = json!(6);
        marker.payload = serde_json::to_vec(&v).unwrap();
        let previous = marker.version;
        marker.version += 1;
        b.store()
            .put_provider_state(marker, Some(previous))
            .unwrap();
        let before = capture(&b);
        assert_eq!(
            read(b.store(), 3, 5, Default::default()).err(),
            Some(ReadError::Identity)
        );
        assert!(MqService::open(b.arc(), Default::default()).is_err());
        assert_eq!(capture(&b), before);
        assert_eq!(s.lock().unwrap().state, old_state);
    }
}

#[test]
fn captured_snapshot_is_stable_after_competing_legacy_or_rich_publication() {
    for sqlite in [false, true] {
        for is_rich in [false, true] {
            let b = Backend::new(sqlite);
            let c = fixture(&b, true);
            if is_rich {
                import(&b);
            }
            let captured = capture(&b);
            if is_rich {
                let mut current = rich(&b);
                get(&mut current.delivery, &c);
                let delta = current
                    .rows
                    .delta(
                        &current.delivery,
                        &c,
                        DeliveryRowIdentity::new(&c, 3, 5).unwrap(),
                    )
                    .unwrap();
                b.store()
                    .mutate_provider_states_atomic(delta.into_parts().0)
                    .unwrap();
            } else {
                let mut queue = captured
                    .iter()
                    .find(|r| r.namespace == QUEUE_NAMESPACE && r.key == "A")
                    .unwrap()
                    .clone();
                let mut value: Value = serde_json::from_slice(&queue.payload).unwrap();
                value["value"]["messages"].as_array_mut().unwrap().remove(0);
                queue.payload = serde_json::to_vec(&value).unwrap();
                queue.version += 1;
                let mut manifest = captured
                    .iter()
                    .find(|r| r.namespace == STATE_NAMESPACE)
                    .unwrap()
                    .clone();
                manifest.version += 1;
                b.store()
                    .mutate_provider_states_atomic(vec![
                        ProviderStateMutation::Put(ProviderStateWrite {
                            expected_version: Some(2),
                            record: queue,
                        }),
                        ProviderStateMutation::Put(ProviderStateWrite {
                            expected_version: Some(2),
                            record: manifest,
                        }),
                    ])
                    .unwrap();
            }
            let after = capture(&b);
            let frozen = decode_records(captured, 3, 5, Default::default()).unwrap();
            match frozen {
                StoredAuthority::Legacy(d) => assert_eq!(d.state.queues["A"].messages.len(), 2),
                StoredAuthority::Rich(r) => assert_eq!(r.delivery.depth(&name("A")), Some(2)),
            }
            match load(&b) {
                StoredAuthority::Legacy(d) => assert_eq!(d.state.queues["A"].messages.len(), 1),
                StoredAuthority::Rich(r) => assert_eq!(r.delivery.depth(&name("A")), Some(1)),
            }
            assert_eq!(capture(&b), after);
        }
    }
}

#[test]
fn physical_and_semantic_budgets_are_independent_finite_and_checked_before_decode() {
    for sqlite in [false, true] {
        let b = Backend::new(sqlite);
        fixture(&b, true);
        import(&b);
        let before = capture(&b);
        let total = before.iter().map(|r| r.payload.len()).sum::<usize>();
        let maxrow = before.iter().map(|r| r.payload.len()).max().unwrap();
        let mut profiles = vec![];
        for records in [1, before.len() - 1, usize::MAX] {
            profiles.push(ReaderLimits {
                records,
                ..Default::default()
            });
        }
        for row_bytes in [1, maxrow - 1, usize::MAX] {
            profiles.push(ReaderLimits {
                row_bytes,
                ..Default::default()
            });
        }
        for total_bytes in [1, total - 1, usize::MAX] {
            profiles.push(ReaderLimits {
                total_bytes,
                ..Default::default()
            });
        }
        profiles.push(ReaderLimits {
            legacy: MqLimits {
                max_replays: 1,
                ..Default::default()
            },
            ..Default::default()
        });
        profiles.push(ReaderLimits {
            rows: DeliveryRowLimits {
                rows: 1,
                ..Default::default()
            },
            ..Default::default()
        });
        profiles.push(ReaderLimits {
            delivery: MqDeliveryLimits {
                depth_per_queue: 1,
                ..Default::default()
            },
            ..Default::default()
        });
        profiles.push(ReaderLimits {
            message: MqMessageLimits {
                body_bytes: 1,
                ..Default::default()
            },
            ..Default::default()
        });
        for limits in profiles {
            assert!(read(b.store(), 3, 5, limits).is_err(), "{limits:?}");
        }
        assert!(
            read(
                b.store(),
                3,
                5,
                ReaderLimits {
                    records: before.len(),
                    row_bytes: maxrow,
                    total_bytes: total,
                    ..Default::default()
                }
            )
            .is_ok()
        );
        assert_eq!(capture(&b), before);
    }
    assert_eq!(ReaderLimits::default().total_bytes, 128 << 20);
}

#[test]
fn retained_replay_and_valid_rich_delivery_can_exceed_one_64mib_side() {
    let b = Backend::new(false);
    let c = fixture(&b, true);
    let mut replay = b
        .store()
        .get_provider_state(REPLAY_NAMESPACE, "old-open")
        .unwrap()
        .unwrap();
    replay.version += 1;
    // Retained, valid historical whitespace is part of the physical footprint.
    replay.payload.resize(55 << 20, b' ');
    b.store()
        .put_provider_state(replay.clone(), Some(2))
        .unwrap();
    import(&b);
    let mut r = rich(&b);
    let mut message = get(&mut r.delivery, &c);
    message.body = vec![255; 1 << 20];
    for _ in 0..3 {
        r.delivery
            .put_one(&c, &name("A"), message.clone(), None)
            .unwrap();
    }
    let delta = r
        .rows
        .delta(&r.delivery, &c, DeliveryRowIdentity::new(&c, 3, 5).unwrap())
        .unwrap();
    b.store()
        .mutate_provider_states_atomic(delta.into_parts().0)
        .unwrap();
    let before = capture(&b);
    assert!(before.iter().map(|r| r.payload.len()).sum::<usize>() > 64 << 20);
    let restored = rich(&b);
    assert_eq!(
        restored.delivery.encode_live_checkpoint().unwrap(),
        r.delivery.encode_live_checkpoint().unwrap()
    );
    assert!(restored.retained_records.contains(&replay));
    assert_eq!(
        read(
            b.store(),
            3,
            5,
            ReaderLimits {
                total_bytes: 64 << 20,
                ..Default::default()
            }
        )
        .err(),
        Some(ReadError::Bounds)
    );
    assert_eq!(capture(&b), before);
}

#[test]
fn exact_source_ceiling_import_with_larger_marker_remains_readable() {
    let b = Backend::new(false);
    let c = MqObjectCatalog::new(
        MqQueueManagerDefinition {
            name: name("EMPTY.QM"),
            default_transmission_queue: None,
        },
        vec![],
        Default::default(),
    )
    .unwrap();
    let manifest=ProviderStateRecord {namespace:STATE_NAMESPACE.into(),key:STATE_KEY.into(),version:1,
        payload:br#"{"schema_version":"mainframe-env.mq-row-store@1","definitions":null,"next_handle":1}"#.to_vec()};
    let catalog = object(
        CATALOG_NAMESPACE,
        CATALOG_KEY,
        json!(String::from_utf8(c.encode().unwrap()).unwrap()),
    );
    let mut replay = object(
        REPLAY_NAMESPACE,
        "historical-receipt",
        json!({"request_sha256":vec![13;32],"completion_code":0,"reason_code":0,
        "handle":null,"message":[],"message_id":null,"correlation_id":null,"trigger_program":null}),
    );
    replay.payload.resize(
        (64 << 20) - manifest.payload.len() - catalog.payload.len(),
        b' ',
    );
    for record in [manifest, catalog, replay.clone()] {
        b.store().put_provider_state(record, None).unwrap();
    }
    assert_eq!(
        capture(&b).iter().map(|r| r.payload.len()).sum::<usize>(),
        64 << 20
    );
    assert!(matches!(load(&b), StoredAuthority::Legacy(_)));
    import(&b);
    let before = capture(&b);
    assert!(
        before
            .iter()
            .filter(|r| !r.namespace.starts_with(PREFIX))
            .map(|r| r.payload.len())
            .sum::<usize>()
            > 64 << 20
    );
    let restored = rich(&b);
    assert!(restored.retained_records.contains(&replay));
    assert_eq!(restored.catalog.encode().unwrap(), c.encode().unwrap());
    assert_eq!(capture(&b), before);
}

#[test]
fn live_pending_finalized_expiry_and_nonpersistent_policy_remain_original() {
    for sqlite in [false, true] {
        let mut b = Backend::new(sqlite);
        let c = fixture(&b, true);
        import(&b);
        let mut r = rich(&b);
        let prototype = get(&mut r.delivery, &c);
        let mut nonpersistent = prototype.clone();
        nonpersistent.body = b"live-only".to_vec();
        nonpersistent.descriptor.persistence = MqPersistence::NonPersistent;
        r.delivery
            .put_one(&c, &name("A"), nonpersistent.clone(), None)
            .unwrap();
        nonpersistent.descriptor.expiry = mainframe_env_host_api::MqExpiry::RelativeHostTicks(2);
        r.delivery
            .put_one(&c, &name("A"), nonpersistent, None)
            .unwrap();
        r.delivery
            .put_one(&c, &name("B"), prototype.clone(), Some(31))
            .unwrap();
        r.delivery
            .put_one(&c, &name("B"), prototype, Some(32))
            .unwrap();
        r.delivery.backout(32).unwrap();
        r.delivery.advance_tick(2).unwrap();
        let live = r.delivery.encode_live_checkpoint().unwrap();
        let cold = r.delivery.encode().unwrap();
        b.store()
            .mutate_provider_states_atomic(
                r.rows
                    .delta(&r.delivery, &c, DeliveryRowIdentity::new(&c, 3, 5).unwrap())
                    .unwrap()
                    .into_parts()
                    .0,
            )
            .unwrap();
        drop(r);
        b.reopen();
        let before = capture(&b);
        let mut r = rich(&b);
        assert_eq!(r.delivery.encode_live_checkpoint().unwrap(), live);
        assert_eq!(r.delivery.encode().unwrap(), cold);
        assert_eq!(r.delivery.depth(&name("A")), Some(2));
        assert_eq!(r.delivery.depth(&name("B")), Some(0));
        let restarted = MqDeliveryKernel::decode(
            &cold,
            &c,
            Default::default(),
            Default::default(),
            MqPersistence::Persistent,
        )
        .unwrap();
        assert_eq!(restarted.depth(&name("A")), Some(1));
        r.delivery.commit(31).unwrap();
        assert_eq!(r.delivery.depth(&name("B")), Some(1));
        let committed = r.delivery.encode_live_checkpoint().unwrap();
        r.delivery.commit(31).unwrap();
        assert_eq!(r.delivery.encode_live_checkpoint().unwrap(), committed);
        assert_eq!(
            r.delivery.commit(32).unwrap(),
            mainframe_env_host_api::MqDeliveryOutcome::UnknownOutcome
        );
        assert_eq!(r.delivery.encode_live_checkpoint().unwrap(), committed);
        assert_eq!(capture(&b), before);
    }
}

fn invocation() -> Invocation {
    let l = InvocationLimits::default();
    let mut invocation = Invocation::new(
        RequestId::new("request-reader", l).unwrap(),
        ExecutionId::new("execution-reader", l).unwrap(),
        RunUnitId::new("run-reader", l).unwrap(),
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
        TraceId::new("trace-reader", l).unwrap(),
        IdempotencyKey::new("invocation-reader", l).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        l,
    )
    .unwrap();
    let (binding, schema) = crate::host_context::host_context_contract();
    invocation.bindings.insert(
        binding.into(),
        mainframe_env_execution_api::BoundedPayload::new(
            schema,
            b"other-bindings|queue-manager".to_vec(),
            l,
        )
        .unwrap(),
    );
    invocation
}

#[test]
fn actual_protected_replay_retains_owner_binding_and_physical_bytes() {
    for sqlite in [false, true] {
        let b = Backend::new(sqlite);
        fixture(&b, false);
        let s = MqService::open(b.arc(), Default::default()).unwrap();
        s.execute(
            &invocation(),
            &MqRequest {
                operation: MqOperation::Commit,
                queue: None,
                handle: None,
                options: 0,
                message: vec![],
                message_id: None,
                correlation_id: None,
                wait_ticks: 0,
                max_message_bytes: 1024,
                mutation: Some(mainframe_env_host_api::Mutation {
                    sequence: 1,
                    idempotency_key: IdempotencyKey::new("reader-effect", Default::default())
                        .unwrap(),
                    transaction: None,
                }),
            },
        )
        .unwrap();
        let before = b
            .store()
            .get_provider_state(REPLAY_NAMESPACE, "reader-effect")
            .unwrap()
            .unwrap();
        let descriptor = crate::describe_mq_replay_row(&before, None, Default::default()).unwrap();
        assert_eq!(
            descriptor.retention,
            crate::MqReplayRetentionState::PendingProtected
        );
        let StoredAuthority::Legacy(d) = load(&b) else {
            panic!("legacy expected")
        };
        assert_eq!(
            d.state.replay["reader-effect"].owner_execution.as_deref(),
            Some("execution-reader")
        );
        import(&b);
        let r = rich(&b);
        assert!(r.retained_records.contains(&before));
        assert_eq!(
            r.versions[&(before.namespace.clone(), before.key.clone())],
            before.version
        );
        assert_eq!(
            r.replay["reader-effect"].owner_run_unit.as_deref(),
            Some("run-reader")
        );
        assert_eq!(
            crate::describe_mq_replay_row(&before, None, Default::default()).unwrap(),
            descriptor
        );
        for field in [
            "owner_execution",
            "owner_run_unit",
            "result_sha256",
            "retention_binding_sha256",
        ] {
            let mut bad = capture(&b);
            edit(&mut bad, REPLAY_NAMESPACE, "reader-effect", |v| {
                v["value"][field] = if field.ends_with("sha256") {
                    json!(vec![0; 32])
                } else {
                    json!("invented")
                };
            });
            reject(bad);
        }
        assert_eq!(
            b.store()
                .get_provider_state(REPLAY_NAMESPACE, "reader-effect")
                .unwrap(),
            Some(before)
        );
    }
}

#[test]
fn physical_historical_provenance_advances_and_rich_corruption_are_read_only() {
    for sqlite in [false, true] {
        let b = Backend::new(sqlite);
        fixture(&b, true);
        import(&b);
        let before = capture(&b);
        let batch = before
            .iter()
            .filter(|r| r.namespace == STATE_NAMESPACE || r.namespace == CATALOG_NAMESPACE)
            .map(|r| {
                let mut record = r.clone();
                record.version += 1;
                ProviderStateMutation::Put(ProviderStateWrite {
                    record,
                    expected_version: Some(r.version),
                })
            })
            .collect();
        b.store().mutate_provider_states_atomic(batch).unwrap();
        assert_eq!(rich(&b).marker.source_manifest_version, 2);
        let original = capture(&b);
        for (ns, key, field) in [
            ("mq-delivery-live-v1-meta", "state", "unknown"),
            ("mq-delivery-live-v1-queue", "A", "unknown"),
        ] {
            let mut bad = original.clone();
            edit(&mut bad, ns, key, |v| {
                v["value"][field] = json!(0);
            });
            reject(bad);
        }
        for (ns, key) in [
            (CATALOG_NAMESPACE, CATALOG_KEY),
            ("mq-delivery-live-v1-meta", "state"),
            ("mq-delivery-live-v1-queue", "A"),
        ] {
            let mut bad = original.clone();
            edit(&mut bad, ns, key, |v| {
                v["schema_version"] = json!("future");
            });
            reject(bad);
            let mut bad = original.clone();
            edit(&mut bad, ns, key, |v| {
                v["object_key"] = json!("wrong");
            });
            reject(bad);
            let mut bad = original.clone();
            bad.iter_mut()
                .find(|r| r.namespace == ns && r.key == key)
                .unwrap()
                .payload = b"{".to_vec();
            reject(bad);
        }
        let mut bad = original.clone();
        edit(&mut bad, CATALOG_NAMESPACE, CATALOG_KEY, |v| {
            v["value"] = json!("{}");
        });
        reject(bad);
        let mut orphan = original.clone();
        orphan.retain(|r| r.namespace != STATE_NAMESPACE);
        reject(orphan);
        assert_eq!(capture(&b), original);
    }
}
