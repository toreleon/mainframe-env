use super::*;
use crate::{MqLocalQueueUsage, MqObjectDefinition, MqObjectLimits, MqQueueManagerDefinition};
use mainframe_env_store::{MemoryStore, SqliteStateStore};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DB: AtomicU64 = AtomicU64::new(1);

struct Backend {
    store: Option<Box<dyn ProviderStateStore>>,
    path: Option<PathBuf>,
}

impl Backend {
    fn new(sqlite: bool) -> Self {
        Self::bounded(sqlite, 1 << 20, 256)
    }

    fn bounded(sqlite: bool, max_bytes: usize, max_rows: usize) -> Self {
        if !sqlite {
            return Self {
                store: Some(Box::new(MemoryStore::new(
                    mainframe_env_store::StoreLimits {
                        max_blob_bytes: max_bytes,
                        max_provider_state: max_rows,
                        ..Default::default()
                    },
                ))),
                path: None,
            };
        }
        let path = std::env::temp_dir().join(format!(
            "mq-delivery-rows-{}-{}.sqlite",
            std::process::id(),
            NEXT_DB.fetch_add(1, Ordering::Relaxed)
        ));
        assert!(
            !path.exists(),
            "fixture must not reuse another test's database"
        );
        let store = SqliteStateStore::open(
            &format!("sqlite://{}?mode=rwc", path.display()),
            max_bytes,
            max_rows,
        )
        .unwrap();
        Self {
            store: Some(Box::new(store)),
            path: Some(path),
        }
    }
    fn store(&self) -> &dyn ProviderStateStore {
        &**self.store.as_ref().unwrap()
    }
    fn reopen(&mut self) {
        if let Some(path) = &self.path {
            drop(self.store.take());
            self.store = Some(Box::new(
                SqliteStateStore::open(
                    &format!("sqlite://{}?mode=rw", path.display()),
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
        if let Some(path) = &self.path {
            for file in [
                path.clone(),
                PathBuf::from(format!("{}-wal", path.display())),
                PathBuf::from(format!("{}-shm", path.display())),
            ] {
                if file.exists() {
                    std::fs::remove_file(file).unwrap();
                }
            }
        }
    }
}

fn name(value: &str) -> MqObjectName {
    MqObjectName::new(value).unwrap()
}

fn catalog() -> MqObjectCatalog {
    MqObjectCatalog::new(
        MqQueueManagerDefinition {
            name: name("QM"),
            default_transmission_queue: None,
        },
        ["A", "B"]
            .into_iter()
            .map(|q| MqObjectDefinition::LocalQueue {
                name: name(q),
                usage: MqLocalQueueUsage::Normal,
                trigger_process: None,
            })
            .collect(),
        MqObjectLimits::default(),
    )
    .unwrap()
}

fn kernel(c: &MqObjectCatalog) -> MqDeliveryKernel {
    MqDeliveryKernel::new(
        c,
        MqDeliveryLimits::default(),
        MqMessageLimits::default(),
        MqPersistence::Persistent,
    )
    .unwrap()
}

fn message(body: &[u8]) -> MqMessage {
    MqMessage {
        descriptor: MqMessageDescriptor {
            identifiers: MqMessageIdentifiers::default(),
            format: Some("FMT".into()),
            expiry: MqExpiry::Unlimited,
            persistence: MqPersistence::Persistent,
            priority: MqPriority::QueueDefault,
            ordering: MqMessageOrdering::default(),
        },
        body: body.into(),
        properties: vec![MqMessageProperty {
            name: "p".into(),
            kind: MqPropertyType::String,
            value: b"value".to_vec(),
        }],
    }
}

fn get(
    k: &mut MqDeliveryKernel,
    c: &MqObjectCatalog,
    q: &str,
    mode: MqGetMode,
    unit: Option<u64>,
) -> MqDeliveryGet {
    k.get(
        c,
        &name(q),
        &MqGetContract {
            selection: Default::default(),
            mode,
            wait: MqWait::NoWait,
            truncation: MqTruncation::Reject,
            buffer_capacity: 1024,
        },
        unit,
    )
    .unwrap()
}

fn identity(c: &MqObjectCatalog) -> DeliveryRowIdentity {
    DeliveryRowIdentity::new(c, 7, 11).unwrap()
}

fn load(b: &Backend, c: &MqObjectCatalog) -> (DeliveryRows, MqDeliveryKernel) {
    DeliveryRows::load(
        b.store(),
        c,
        identity(c),
        DeliveryRowLimits::default(),
        MqDeliveryLimits::default(),
        MqMessageLimits::default(),
        MqPersistence::Persistent,
    )
    .unwrap()
}

fn publish(b: &Backend, delta: DeliveryRowDelta) -> DeliveryRows {
    let (mutations, next) = delta.into_parts();
    b.store().mutate_provider_states_atomic(mutations).unwrap();
    next
}

fn initialize(b: &Backend, c: &MqObjectCatalog, k: &MqDeliveryKernel) -> DeliveryRows {
    publish(
        b,
        DeliveryRows::initialize(b.store(), k, c, identity(c), DeliveryRowLimits::default())
            .unwrap(),
    )
}

fn records(b: &Backend) -> Vec<ProviderStateRecord> {
    b.store().list_provider_state_prefix(PREFIX, 100).unwrap()
}

fn rich(c: &MqObjectCatalog) -> MqDeliveryKernel {
    let mut k = kernel(c);
    k.advance_tick(4).unwrap();
    let mut first = message(b"ab");
    first.descriptor.identifiers = MqMessageIdentifiers {
        message_id: Some(vec![5]),
        correlation_id: Some(vec![6]),
        group_id: Some(vec![7]),
    };
    first.descriptor.ordering = MqMessageOrdering {
        group_sequence: Some(1),
        segment_offset: Some(0),
        last_segment: false,
        segmentation_allowed: true,
        last_in_group: false,
    };
    first.descriptor.expiry = MqExpiry::RelativeHostTicks(20);
    let mut second = first.clone();
    second.body = b"cd".to_vec();
    second.descriptor.ordering.segment_offset = Some(2);
    second.descriptor.ordering.last_segment = true;
    second.descriptor.ordering.last_in_group = true;
    k.put_one(c, &name("A"), first, None).unwrap();
    k.put_one(c, &name("A"), second, None).unwrap();
    k.put_one(c, &name("A"), message(b"staged"), Some(7))
        .unwrap();
    assert_eq!(
        get(&mut k, c, "A", MqGetMode::Remove, Some(7))
            .message
            .unwrap()
            .body,
        b"ab"
    );
    let mut nonpersistent = message(b"volatile");
    nonpersistent.descriptor.persistence = MqPersistence::NonPersistent;
    nonpersistent.descriptor.expiry = MqExpiry::RelativeHostTicks(10);
    k.put_one(c, &name("B"), nonpersistent, None).unwrap();
    assert_eq!(
        get(&mut k, c, "B", MqGetMode::BrowseFirst, None).cursor,
        Some(1)
    );
    get(&mut k, c, "B", MqGetMode::Remove, Some(8));
    k.put_one(c, &name("B"), message(b"keep"), None).unwrap();
    k.put_one(c, &name("B"), message(b"discard"), Some(9))
        .unwrap();
    k.backout(9).unwrap();
    k.put_one(c, &name("B"), message(b"final"), Some(10))
        .unwrap();
    k.commit(10).unwrap();
    k
}

#[test]
fn memory_and_sqlite_live_reopen_preserve_every_projection_field() {
    for sqlite in [false, true] {
        let mut b = Backend::new(sqlite);
        let c = catalog();
        let k = rich(&c);
        let live = k.encode_live_checkpoint().unwrap();
        let cold = k.encode().unwrap();
        let published = initialize(&b, &c, &k);
        assert_eq!(published.records.len(), 8); // meta + 2 queues + 2 units + 2 decisions + cursor
        assert_eq!(published.metadata.counts, [2, 2, 2, 1]);
        assert!(records(&b).iter().all(|r| r.payload.len() < live.len()));
        b.reopen();
        let (loaded, mut restored) = load(&b, &c);
        assert_eq!(loaded.records, published.records);
        assert_eq!(restored.encode_live_checkpoint().unwrap(), live);
        assert_eq!(restored.encode().unwrap(), cold);
        assert_eq!(
            restored,
            MqDeliveryKernel::decode_live_checkpoint(
                &live,
                &c,
                MqDeliveryLimits::default(),
                MqMessageLimits::default(),
                MqPersistence::Persistent
            )
            .unwrap()
        );
        assert_eq!(
            get(
                &mut restored,
                &c,
                "B",
                MqGetMode::BrowseNext { cursor: 1 },
                None
            )
            .message
            .unwrap()
            .body,
            b"keep"
        );
    }
}

#[test]
fn memory_and_sqlite_commit_backout_and_duplicate_replay_survive_reopen() {
    for sqlite in [false, true] {
        let mut b = Backend::new(sqlite);
        let c = catalog();
        let k = rich(&c);
        initialize(&b, &c, &k);
        b.reopen();
        let (rows, mut resumed) = load(&b, &c);
        assert_eq!(resumed.commit(7), Ok(MqDeliveryOutcome::Accepted));
        assert_eq!(resumed.backout(8), Ok(MqDeliveryOutcome::Rejected));
        publish(&b, rows.delta(&resumed, &c, identity(&c)).unwrap());
        b.reopen();
        let (rows, mut resumed) = load(&b, &c);
        let before = resumed.clone();
        assert_eq!(resumed.commit(7), Ok(MqDeliveryOutcome::DuplicatePossible));
        assert_eq!(resumed.backout(7), Ok(MqDeliveryOutcome::DuplicatePossible));
        assert_eq!(resumed.backout(8), Ok(MqDeliveryOutcome::Rejected));
        assert_eq!(resumed.commit(8), Ok(MqDeliveryOutcome::UnknownOutcome));
        assert_eq!(resumed, before);
        let delta = rows.delta(&resumed, &c, identity(&c)).unwrap();
        assert_eq!(delta.mutations().len(), 1); // the metadata fence is always present
        publish(&b, delta);
        assert_eq!(
            get(&mut resumed, &c, "A", MqGetMode::Remove, None)
                .message
                .unwrap()
                .body,
            b"cd"
        );
        assert_eq!(
            get(&mut resumed, &c, "A", MqGetMode::Remove, None)
                .message
                .unwrap()
                .body,
            b"staged"
        );
        assert_eq!(
            get(&mut resumed, &c, "A", MqGetMode::Remove, None).disposition,
            MqGetDisposition::NoMessage
        );
        assert_eq!(
            get(&mut resumed, &c, "B", MqGetMode::Remove, None)
                .message
                .unwrap()
                .body,
            b"volatile"
        );
    }
}

#[test]
fn memory_and_sqlite_stale_metadata_cas_rolls_back_disjoint_object_writes() {
    for sqlite in [false, true] {
        let b = Backend::new(sqlite);
        let c = catalog();
        let k = kernel(&c);
        let stale = initialize(&b, &c, &k);
        let mut winner = k.clone();
        winner
            .put_one(&c, &name("A"), message(b"winner"), None)
            .unwrap();
        let current = publish(&b, stale.delta(&winner, &c, identity(&c)).unwrap());
        assert_eq!(current.records[&key(QUEUE, "B")].version, 1);
        assert_eq!(current.records[&key(QUEUE, "A")].version, 2);
        assert_eq!(current.records[&key(META, META_KEY)].version, 2);
        let before = records(&b);
        let mut loser = k.clone();
        loser
            .put_one(&c, &name("B"), message(b"loser"), None)
            .unwrap();
        let delta = stale.delta(&loser, &c, identity(&c)).unwrap();
        assert!(
            matches!(delta.mutations().last(), Some(ProviderStateMutation::Put(w)) if w.record.namespace == META)
        );
        assert_eq!(
            b.store()
                .mutate_provider_states_atomic(delta.mutations().to_vec()),
            Err(StoreError::Conflict)
        );
        assert_eq!(records(&b), before);
        assert_eq!(
            load(&b, &c).1.encode_live_checkpoint().unwrap(),
            winner.encode_live_checkpoint().unwrap()
        );
        let fresh = publish(&b, current.delta(&winner, &c, identity(&c)).unwrap());
        assert_eq!(fresh.records[&key(META, META_KEY)].version, 3);
        assert_eq!(fresh.records[&key(QUEUE, "A")].version, 2);
        assert_eq!(fresh.records[&key(QUEUE, "B")].version, 1);
    }
}

#[test]
fn memory_and_sqlite_composed_batch_failure_preserves_delivery_and_owner_rows() {
    for sqlite in [false, true] {
        let b = Backend::new(sqlite);
        let c = catalog();
        let mut k = kernel(&c);
        let rows = initialize(&b, &c, &k);
        let sentinel = ProviderStateRecord {
            namespace: "mq-test-owner-replay".into(),
            key: "effect".into(),
            version: 1,
            payload: b"prior-owner-record".to_vec(),
        };
        b.store()
            .put_provider_state(sentinel.clone(), None)
            .unwrap();
        k.put_one(&c, &name("A"), message(b"new"), Some(7)).unwrap();
        let before = records(&b);
        let mut batch = rows
            .delta(&k, &c, identity(&c))
            .unwrap()
            .mutations()
            .to_vec();
        let mut replacement = sentinel.clone();
        replacement.version = 100;
        batch.push(ProviderStateMutation::Put(ProviderStateWrite {
            record: replacement,
            expected_version: Some(99),
        }));
        assert_eq!(
            b.store().mutate_provider_states_atomic(batch),
            Err(StoreError::Conflict)
        );
        assert_eq!(records(&b), before);
        assert_eq!(
            b.store()
                .get_provider_state(&sentinel.namespace, &sentinel.key)
                .unwrap(),
            Some(sentinel.clone())
        );
        let mut batch = rows
            .delta(&k, &c, identity(&c))
            .unwrap()
            .mutations()
            .to_vec();
        let mut replacement = sentinel.clone();
        replacement.version = 2;
        replacement.payload = b"published-owner-record".to_vec();
        batch.push(ProviderStateMutation::Put(ProviderStateWrite {
            record: replacement.clone(),
            expected_version: Some(1),
        }));
        b.store().mutate_provider_states_atomic(batch).unwrap();
        assert_eq!(load(&b, &c).1.unit_outcome(7), MqDeliveryOutcome::Pending);
        assert_eq!(
            b.store()
                .get_provider_state(&sentinel.namespace, &sentinel.key)
                .unwrap(),
            Some(replacement)
        );
    }
}

#[test]
fn memory_and_sqlite_fence_and_exact_catalog_generation_fail_closed() {
    for sqlite in [false, true] {
        let mut b = Backend::new(sqlite);
        let c = catalog();
        let k = rich(&c);
        let rows = initialize(&b, &c, &k);
        for expected in [
            DeliveryRowIdentity::new(&c, 8, 11).unwrap(),
            DeliveryRowIdentity::new(&c, 7, 12).unwrap(),
        ] {
            assert!(matches!(
                DeliveryRows::load(
                    b.store(),
                    &c,
                    expected,
                    DeliveryRowLimits::default(),
                    MqDeliveryLimits::default(),
                    MqMessageLimits::default(),
                    MqPersistence::Persistent
                ),
                Err(DeliveryRowError::Identity)
            ));
        }
        // Same manager and local queue set, different definition bytes.
        let altered = MqObjectCatalog::new(
            c.queue_manager().clone(),
            c.definitions()
                .cloned()
                .chain([MqObjectDefinition::Topic {
                    name: name("extra"),
                }])
                .collect(),
            MqObjectLimits::default(),
        )
        .unwrap();
        assert!(matches!(
            DeliveryRows::load(
                b.store(),
                &altered,
                identity(&altered),
                DeliveryRowLimits::default(),
                MqDeliveryLimits::default(),
                MqMessageLimits::default(),
                MqPersistence::Persistent
            ),
            Err(DeliveryRowError::Identity)
        ));
        assert!(matches!(
            rows.delta(&k, &c, DeliveryRowIdentity::new(&c, 8, 11).unwrap()),
            Err(DeliveryRowError::Identity)
        ));
        assert!(matches!(
            rows.delta(&k, &c, DeliveryRowIdentity::new(&c, 7, 10).unwrap()),
            Err(DeliveryRowError::Identity)
        ));
        let new_fence = DeliveryRowIdentity::new(&c, 7, 12).unwrap();
        publish(&b, rows.delta(&k, &c, new_fence.clone()).unwrap());
        assert_eq!(
            b.store().mutate_provider_states_atomic(
                rows.delta(&k, &c, identity(&c))
                    .unwrap()
                    .mutations()
                    .to_vec()
            ),
            Err(StoreError::Conflict)
        );
        b.reopen();
        assert!(matches!(
            DeliveryRows::load(
                b.store(),
                &c,
                identity(&c),
                DeliveryRowLimits::default(),
                MqDeliveryLimits::default(),
                MqMessageLimits::default(),
                MqPersistence::Persistent
            ),
            Err(DeliveryRowError::Identity)
        ));
        assert_eq!(
            DeliveryRows::load(
                b.store(),
                &c,
                new_fence,
                DeliveryRowLimits::default(),
                MqDeliveryLimits::default(),
                MqMessageLimits::default(),
                MqPersistence::Persistent
            )
            .unwrap()
            .1
            .encode_live_checkpoint()
            .unwrap(),
            k.encode_live_checkpoint().unwrap()
        );
    }
}

#[test]
fn memory_and_sqlite_expiry_nonpersistent_live_and_cold_policies_remain_distinct() {
    for sqlite in [false, true] {
        let mut b = Backend::new(sqlite);
        let c = catalog();
        let k = rich(&c);
        initialize(&b, &c, &k);
        b.reopen();
        let (rows, mut live) = load(&b, &c);
        let mut cold = MqDeliveryKernel::decode(
            &live.encode().unwrap(),
            &c,
            MqDeliveryLimits::default(),
            MqMessageLimits::default(),
            MqPersistence::Persistent,
        )
        .unwrap();
        assert_eq!(cold.unit_outcome(7), MqDeliveryOutcome::UnknownOutcome);
        assert_eq!(cold.unit_outcome(8), MqDeliveryOutcome::UnknownOutcome);
        assert_eq!(
            get(&mut cold, &c, "A", MqGetMode::Remove, None)
                .message
                .unwrap()
                .body,
            b"ab"
        );
        assert_eq!(
            get(&mut cold, &c, "B", MqGetMode::Remove, None)
                .message
                .unwrap()
                .body,
            b"keep"
        );
        live.advance_tick(14).unwrap(); // nonpersistent pending get expires
        assert_eq!(live.unit_outcome(8), MqDeliveryOutcome::Pending); // empty UOW retained
        publish(&b, rows.delta(&live, &c, identity(&c)).unwrap());
        b.reopen();
        let (rows, mut live) = load(&b, &c);
        live.recover_backout().unwrap();
        assert_eq!(live.unit_outcome(8), MqDeliveryOutcome::Rejected);
        assert_eq!(
            get(&mut live, &c, "B", MqGetMode::Remove, None)
                .message
                .unwrap()
                .body,
            b"keep"
        );
        publish(&b, rows.delta(&live, &c, identity(&c)).unwrap());
        b.reopen();
        let (_, live) = load(&b, &c);
        assert_eq!(live.unit_outcome(7), MqDeliveryOutcome::Rejected);
        assert_eq!(live.unit_outcome(8), MqDeliveryOutcome::Rejected);
    }
}

fn restore_raw(
    records: Vec<ProviderStateRecord>,
    c: &MqObjectCatalog,
    limits: DeliveryRowLimits,
) -> Result<(DeliveryRows, MqDeliveryKernel), DeliveryRowError> {
    DeliveryRows::restore(
        records,
        c,
        identity(c),
        limits,
        MqDeliveryLimits::default(),
        MqMessageLimits::default(),
        MqPersistence::Persistent,
    )
}

// Corruption fixtures may recompute the row-set digest to reach the sole
// checkpoint semantic validator, rather than only testing checksum failure.
fn resign(records: &mut [ProviderStateRecord]) {
    let map: Records = records
        .iter()
        .map(|r| ((r.namespace.clone(), r.key.clone()), r.clone()))
        .collect();
    let record = records
        .iter_mut()
        .find(|r| r.namespace == META && r.key == META_KEY)
        .unwrap();
    let mut metadata: Metadata = decode(record).unwrap();
    metadata.counts = counts(&map);
    metadata.rows_sha256 = digest_rows(&map, false);
    record.payload = encode_object_row(META_KEY, &metadata).unwrap();
}

fn edit(record: &mut ProviderStateRecord, f: impl FnOnce(&mut serde_json::Value)) {
    let mut value: serde_json::Value = serde_json::from_slice(&record.payload).unwrap();
    f(&mut value);
    record.payload = serde_json::to_vec(&value).unwrap();
}

#[test]
fn strict_envelopes_and_reused_checkpoint_validation_reject_malformed_rows() {
    let b = Backend::new(false);
    let c = catalog();
    initialize(&b, &c, &rich(&c));
    let source = records(&b);
    for case in 0..17 {
        let mut bad = source.clone();
        let a = bad
            .iter_mut()
            .find(|r| r.namespace == QUEUE && r.key == "A")
            .unwrap();
        match case {
            0 => edit(a, |v| {
                v["unexpected"] = true.into();
            }),
            1 => edit(a, |v| {
                v["object_key"] = "B".into();
            }),
            2 => edit(a, |v| {
                v["schema_version"] = "unknown@2".into();
            }),
            3 => edit(a, |v| {
                v.as_object_mut().unwrap().remove("value");
            }),
            4 => {
                let original = String::from_utf8(a.payload.clone()).unwrap();
                a.payload = original
                    .replacen('{', "{\"object_key\":\"A\",", 1)
                    .into_bytes();
            }
            5 => edit(a, |v| {
                v["value"]["messages"][0]["message"]["message_id"] = serde_json::Value::Null;
            }),
            6 => edit(a, |v| {
                v["value"]["messages"][0]["expires_at"] = 4.into();
            }),
            7 => edit(a, |v| {
                v["value"]["messages"][0]["message"]["group_sequence"] = 0.into();
            }),
            8 => edit(a, |v| {
                v["value"]["messages"][0]["message"]["body"] = serde_json::json!([256]);
            }),
            9 => edit(a, |v| {
                v["value"]["messages"][0]["message"]["body"] = serde_json::json!([]);
            }),
            10 => edit(a, |v| {
                let duplicate = v["value"]["messages"][0].clone();
                v["value"]["messages"]
                    .as_array_mut()
                    .unwrap()
                    .push(duplicate);
            }),
            11 => a.version = 0,
            12 => a.version = i64::MAX as u64 + 1,
            13 => {
                let p = bad.iter_mut().find(|r| r.namespace == PENDING).unwrap();
                edit(p, |v| {
                    v["value"]["operations"][0]["queue"] = "MISSING".into();
                });
            }
            14 => {
                let p = bad.iter_mut().find(|r| r.namespace == PENDING).unwrap();
                edit(p, |v| {
                    v["value"]["operations"][0]["entry"]["id"] = 2.into();
                });
            }
            15 => {
                let cursor = bad.iter_mut().find(|r| r.namespace == CURSOR).unwrap();
                edit(cursor, |v| {
                    v["value"]["entry_id"] = 3.into(); // staged put cannot have a cursor
                    v["value"]["queue"] = "A".into();
                });
            }
            16 => {
                let finality = bad.iter_mut().find(|r| r.namespace == FINAL).unwrap();
                finality.key = number_key(7);
                edit(finality, |v| {
                    v["object_key"] = number_key(7).into();
                    v["value"]["unit"] = 7.into();
                });
            }
            _ => unreachable!(),
        }
        resign(&mut bad);
        assert!(
            restore_raw(bad, &c, DeliveryRowLimits::default()).is_err(),
            "accepted corruption case {case}"
        );
        assert_eq!(records(&b), source);
    }
}

#[test]
fn metadata_membership_order_and_identity_are_strict_and_bounded() {
    let b = Backend::new(false);
    let c = catalog();
    initialize(&b, &c, &rich(&c));
    let source = records(&b);
    for case in 0..10 {
        let mut bad = source.clone();
        match case {
            0 => {
                bad.retain(|r| r.namespace != META);
            }
            1 => {
                bad.retain(|r| !(r.namespace == QUEUE && r.key == "A"));
                resign(&mut bad);
            }
            2 => {
                bad.push(source[0].clone());
            }
            3 => {
                bad[0].namespace = format!("{PREFIX}unknown");
            }
            4 => {
                let m = bad.iter_mut().find(|r| r.namespace == META).unwrap();
                edit(m, |v| {
                    v["value"]["extra"] = true.into();
                });
            }
            5 => {
                let m = bad.iter_mut().find(|r| r.namespace == META).unwrap();
                edit(m, |v| {
                    v["value"]["schema_version"] = "unknown@2".into();
                });
            }
            6 => {
                let m = bad.iter_mut().find(|r| r.namespace == META).unwrap();
                edit(m, |v| {
                    v["value"]["next_id"] = 0.into();
                });
            }
            7 => {
                let m = bad.iter_mut().find(|r| r.namespace == META).unwrap();
                edit(m, |v| {
                    v["value"]["default_persistent"] = false.into();
                });
            }
            8 => {
                bad.reverse();
            } // row scan order is not semantic order
            9 => {
                let cursor = bad.iter_mut().find(|r| r.namespace == CURSOR).unwrap();
                cursor.key = "1".into();
                edit(cursor, |v| {
                    v["object_key"] = "1".into();
                });
                resign(&mut bad);
            }
            _ => unreachable!(),
        }
        let result = restore_raw(bad, &c, DeliveryRowLimits::default());
        if case == 8 {
            assert!(result.is_ok());
        } else {
            assert!(result.is_err(), "accepted metadata case {case}");
        }
    }
    let bytes: usize = source.iter().map(|r| r.payload.len()).sum();
    for limits in [
        DeliveryRowLimits {
            rows: source.len() - 1,
            ..Default::default()
        },
        DeliveryRowLimits {
            row_bytes: source.iter().map(|r| r.payload.len()).max().unwrap() - 1,
            ..Default::default()
        },
        DeliveryRowLimits {
            total_bytes: bytes - 1,
            ..Default::default()
        },
        DeliveryRowLimits {
            rows: 0,
            ..Default::default()
        },
        DeliveryRowLimits {
            rows: 28_194,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            restore_raw(source.clone(), &c, limits),
            Err(DeliveryRowError::Bounds)
        ));
    }
}

#[test]
fn memory_and_sqlite_missing_or_corrupt_rows_never_open_as_empty() {
    for sqlite in [false, true] {
        let b = Backend::new(sqlite);
        let c = catalog();
        let k = kernel(&c);
        let legacy = ProviderStateRecord {
            namespace: "mq-state".into(),
            key: "queues".into(),
            version: 1,
            payload: b"legacy-owner-state".to_vec(),
        };
        b.store().put_provider_state(legacy.clone(), None).unwrap();
        assert!(matches!(
            DeliveryRows::load(
                b.store(),
                &c,
                identity(&c),
                DeliveryRowLimits::default(),
                MqDeliveryLimits::default(),
                MqMessageLimits::default(),
                MqPersistence::Persistent
            ),
            Err(DeliveryRowError::Missing)
        ));
        // Explicit initialization is a manager decision; it never consumes legacy bytes.
        initialize(&b, &c, &k);
        assert_eq!(
            b.store()
                .get_provider_state(&legacy.namespace, &legacy.key)
                .unwrap(),
            Some(legacy)
        );
        let mut bad = b.store().get_provider_state(QUEUE, "A").unwrap().unwrap();
        bad.version += 1;
        bad.payload = b"{\"schema_version\":\"unknown@2\"}".to_vec();
        b.store().put_provider_state(bad, Some(1)).unwrap();
        let before = records(&b);
        assert!(
            DeliveryRows::load(
                b.store(),
                &c,
                identity(&c),
                DeliveryRowLimits::default(),
                MqDeliveryLimits::default(),
                MqMessageLimits::default(),
                MqPersistence::Persistent
            )
            .is_err()
        );
        assert!(matches!(
            DeliveryRows::initialize(
                b.store(),
                &k,
                &c,
                identity(&c),
                DeliveryRowLimits::default()
            ),
            Err(DeliveryRowError::Corrupt)
        ));
        assert_eq!(records(&b), before);
        let meta = b
            .store()
            .get_provider_state(META, META_KEY)
            .unwrap()
            .unwrap();
        b.store()
            .delete_provider_state(META, META_KEY, meta.version)
            .unwrap();
        assert!(matches!(
            DeliveryRows::load(
                b.store(),
                &c,
                identity(&c),
                DeliveryRowLimits::default(),
                MqDeliveryLimits::default(),
                MqMessageLimits::default(),
                MqPersistence::Persistent
            ),
            Err(DeliveryRowError::Corrupt)
        ));
    }
}

#[test]
fn memory_and_sqlite_physical_capacity_and_payload_failures_are_atomic() {
    for sqlite in [false, true] {
        for (bytes, row_count, oversized_body) in [(1 << 20, 3, false), (2048, 256, true)] {
            let b = Backend::bounded(sqlite, bytes, row_count);
            let c = catalog();
            let mut k = kernel(&c);
            let rows = initialize(&b, &c, &k);
            let before = records(&b);
            k.put_one(
                &c,
                &name("A"),
                message(&if oversized_body {
                    vec![7; 3000]
                } else {
                    vec![7]
                }),
                Some(7),
            )
            .unwrap();
            let delta = rows.delta(&k, &c, identity(&c)).unwrap();
            assert!(matches!(
                b.store()
                    .mutate_provider_states_atomic(delta.mutations().to_vec()),
                Err(StoreError::CapacityExceeded | StoreError::PayloadTooLarge)
            ));
            assert_eq!(records(&b), before);
            assert_eq!(
                load(&b, &c).1.unit_outcome(7),
                MqDeliveryOutcome::UnknownOutcome
            );
        }
    }
}

#[test]
fn projection_delta_and_reconstructed_kernel_limits_fail_before_publication() {
    let b = Backend::new(false);
    let c = catalog();
    let k = rich(&c);
    for limits in [
        DeliveryRowLimits {
            rows: 1,
            ..Default::default()
        },
        DeliveryRowLimits {
            row_bytes: 1,
            ..Default::default()
        },
        DeliveryRowLimits {
            total_bytes: 1,
            ..Default::default()
        },
        DeliveryRowLimits {
            mutations: 1,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            DeliveryRows::initialize(b.store(), &k, &c, identity(&c), limits),
            Err(DeliveryRowError::Bounds)
        ));
        assert!(records(&b).is_empty());
    }
    let rows = initialize(&b, &c, &k);
    let source = records(&b);
    for (limits, message_limits) in [
        (
            MqDeliveryLimits {
                depth_per_queue: 1,
                ..Default::default()
            },
            MqMessageLimits::default(),
        ),
        (
            MqDeliveryLimits {
                pending_operations: 1,
                ..Default::default()
            },
            MqMessageLimits::default(),
        ),
        (
            MqDeliveryLimits {
                finalized_units: 1,
                ..Default::default()
            },
            MqMessageLimits::default(),
        ),
        (
            MqDeliveryLimits {
                snapshot_bytes: 1,
                ..Default::default()
            },
            MqMessageLimits::default(),
        ),
        (
            MqDeliveryLimits::default(),
            MqMessageLimits {
                body_bytes: 1,
                ..Default::default()
            },
        ),
    ] {
        assert!(
            DeliveryRows::load(
                b.store(),
                &c,
                identity(&c),
                DeliveryRowLimits::default(),
                limits,
                message_limits,
                MqPersistence::Persistent
            )
            .is_err()
        );
        assert_eq!(records(&b), source);
    }
    let mut new = k.clone();
    new.next_id -= 1;
    assert!(matches!(
        rows.delta(&new, &c, identity(&c)),
        Err(DeliveryRowError::Identity)
    ));
    new = k.clone();
    new.next_cursor -= 1;
    assert!(matches!(
        rows.delta(&new, &c, identity(&c)),
        Err(DeliveryRowError::Identity)
    ));
    new = k.clone();
    new.tick -= 1;
    assert!(matches!(
        rows.delta(&new, &c, identity(&c)),
        Err(DeliveryRowError::Identity)
    ));
    new = k.clone();
    new.finalized.remove(&9);
    assert!(matches!(
        rows.delta(&new, &c, identity(&c)),
        Err(DeliveryRowError::Identity)
    ));
    new = k.clone();
    new.pending.remove(&7);
    assert!(matches!(
        rows.delta(&new, &c, identity(&c)),
        Err(DeliveryRowError::Identity)
    ));
    new = k.clone();
    new.default_persistence = MqPersistence::NonPersistent;
    assert!(matches!(
        rows.delta(&new, &c, identity(&c)),
        Err(DeliveryRowError::Identity)
    ));
    let mut exhausted = source.clone();
    for record in &mut exhausted {
        record.version = i64::MAX as u64;
    }
    resign(&mut exhausted);
    let (rows, normalized) = restore_raw(exhausted, &c, DeliveryRowLimits::default()).unwrap();
    assert!(matches!(
        rows.delta(&normalized, &c, identity(&c)),
        Err(DeliveryRowError::Bounds)
    ));
}

#[test]
fn extracting_typed_projection_keeps_existing_live_and_cold_schema_bytes() {
    let c = catalog();
    let k = kernel(&c);
    assert_eq!(k.encode_live_checkpoint().unwrap(), br#"{"schema_version":"mainframe-env.mq-delivery-live@1","manager":"QM","default_persistent":true,"tick":0,"next_id":1,"next_cursor":1,"queues":[{"name":"A","messages":[]},{"name":"B","messages":[]}],"pending":[],"finalized":[],"cursors":[]}"#);
    assert_eq!(k.encode().unwrap(), br#"{"schema_version":"mainframe-env.mq-delivery@1","manager":"QM","tick":0,"next_id":1,"next_cursor":1,"queues":[{"name":"A","messages":[]},{"name":"B","messages":[]}],"finalized":[]}"#);
}
