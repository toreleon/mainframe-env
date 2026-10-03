//! Real original selected flows; test-owned source ports are not installed JES/LE.
use super::*;
use crate::{MqNativePersistence, MqNativeProducerDefaults, MqNativePutResponse};
#[path = "defaults/guards.rs"]
mod guards;

fn fixture(sqlite: bool, version: i32, persistent: bool) -> ProducerFixture {
    fixture_limits(sqlite, version, persistent, 64 << 20)
}

fn fixture_limits(sqlite: bool, version: i32, persistent: bool, blob: usize) -> ProducerFixture {
    let db = sqlite.then(Database::new);
    let store: Arc<dyn PlatformStore> = db.as_ref().map_or_else(
        || {
            Arc::new(MemoryStore::new(mainframe_env_store::StoreLimits {
                max_audits: 256,
                max_provider_state: 256,
                max_blob_bytes: blob,
                ..Default::default()
            })) as Arc<dyn PlatformStore>
        },
        |db| {
            Arc::new(
                SqliteStateStore::open(
                    &format!("sqlite://{}?mode=rwc", db.0.join("state.sqlite").display()),
                    blob,
                    256,
                )
                .unwrap(),
            )
        },
    );
    let ports = Arc::new(Ports::default());
    let c = catalog(false, 32768);
    let mut attrs = c.native_attributes().unwrap().clone();
    attrs.max_priority = 9;
    attrs.queues[0].producer_defaults = Some(MqNativeProducerDefaults {
        priority: 7,
        persistence: if persistent {
            MqNativePersistence::Persistent
        } else {
            MqNativePersistence::NonPersistent
        },
        response: MqNativePutResponse::Synchronous,
    });
    let c = c.with_native_attributes(attrs).unwrap();
    let f = Fixture::with_producer_setup(store, false, Some((c, version, ports.clone())));
    ProducerFixture { f, ports, db }
}

fn policy_message(version: i32, priority: i32, persistence: i32) -> MqFullMessage {
    let mut m = message(version, false, -77);
    let f = match &mut m.descriptor {
        MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => fields,
    };
    f.priority = priority;
    f.persistence = persistence;
    m
}

#[test]
fn memory_owned_sqlite_default_explicit_put_put1_md1_md2_units_exact_replay_and_stored_policy() {
    for sqlite in [false, true] {
        for version in [1, 2] {
            for one in [false, true] {
                for local in [false, true] {
                    for persistent in [false, true] {
                        for (priority, persistence, expected_priority, expected_persistence) in [
                            (-1, 2, 7, i32::from(persistent)),
                            (5, 0, 5, 0),
                            (0, 1, 0, 1),
                        ] {
                            let f = fixture(sqlite, version, persistent);
                            let c = f.connect();
                            let o = f.open(c);
                            let unit = f.unit();
                            let m = policy_message(version, priority, persistence);
                            let e = f.effect(
                                3,
                                put(
                                    c,
                                    (!one).then_some(o),
                                    m.clone(),
                                    true,
                                    if local {
                                        MqMqiUnitOfWork::Local { unit }
                                    } else {
                                        MqMqiUnitOfWork::NoSyncpoint
                                    },
                                ),
                            );
                            f.seed(&e);
                            let result = f.execute(&e).unwrap();
                            let p = produced(&result);
                            assert_eq!(p.descriptor.fields().priority, priority);
                            assert_eq!(p.descriptor.fields().persistence, persistence);
                            assert_eq!(p.descriptor.fields().backout_count, -77);
                            assert_eq!(p.descriptor.fields().msg_id, m.descriptor.fields().msg_id);
                            assert_eq!(
                                p.outcome,
                                if local {
                                    MqDeliveryOutcome::Pending
                                } else {
                                    MqDeliveryOutcome::Accepted
                                }
                            );
                            let rows = f.rows();
                            assert_eq!(f.execute(&e).unwrap(), result);
                            assert_eq!(f.rows(), rows);
                            let full = match &result.outcome {
                                Ok(HostResult::MqMqi(r)) => r,
                                _ => panic!(),
                            };
                            let encoded = crate::mqi_replay::encode(
                                &full.result,
                                HostLimits::default(),
                                full.limits,
                                1 << 20,
                            )
                            .unwrap();
                            assert!(
                                String::from_utf8_lossy(&encoded)
                                    .contains("mq-mqi-result-storage@5")
                            );
                            // Full original HostResult canonical identity, no standalone substitute.
                            assert_eq!(
                                mainframe_env_host_api::canonical_result_digest(&result.outcome)
                                    .unwrap(),
                                mainframe_env_host_api::canonical_result_digest(
                                    &f.execute(&e).unwrap().outcome
                                )
                                .unwrap()
                            );
                            if local {
                                f.call(
                                    4,
                                    MqMqiRequest::Commit {
                                        connection: c,
                                        unit,
                                    },
                                );
                            }
                            let stored = f.get(c, o, 5, version, false).unwrap();
                            assert_eq!(stored.descriptor.fields().priority, expected_priority);
                            assert_eq!(
                                stored.descriptor.fields().persistence,
                                expected_persistence
                            );
                            assert_eq!(stored.descriptor.fields().backout_count, 0);
                            assert_eq!(stored.body, m.body);
                            assert!(f.get(c, o, 6, version, false).is_none());
                            assert_eq!(f.ports.gmt_calls.load(Ordering::SeqCst), 0);
                            assert_eq!(f.ports.context_calls.load(Ordering::SeqCst), 0);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn default_fifo_keeps_arrival_order_and_backout_does_not_publish_pending_policy() {
    for sqlite in [false, true] {
        let f = fixture(sqlite, 2, true);
        let c = f.connect();
        let o = f.open(c);
        for (seq, priority, body) in [(3, 0, 1), (4, 9, 2), (5, -1, 3)] {
            let mut m = policy_message(2, priority, 2);
            m.body = vec![body];
            f.call(seq, put(c, Some(o), m, true, MqMqiUnitOfWork::NoSyncpoint));
        }
        for (seq, priority, body) in [(6, 0, 1), (7, 9, 2), (8, 7, 3)] {
            let m = f.get(c, o, seq, 2, false).unwrap();
            assert_eq!(m.body, [body]);
            assert_eq!(m.descriptor.fields().priority, priority);
            assert_eq!(m.descriptor.fields().persistence, 1);
        }
        let unit = f.unit();
        f.call(
            9,
            put(
                c,
                Some(o),
                policy_message(2, -1, 2),
                true,
                MqMqiUnitOfWork::Local { unit },
            ),
        );
        f.call(
            10,
            MqMqiRequest::Back {
                connection: c,
                unit,
            },
        );
        assert!(f.get(c, o, 11, 2, false).is_none());
    }
}

#[test]
fn defaults_missing_above_max_and_foreign_unit_refuse_without_state_or_source_mutation() {
    for sqlite in [false, true] {
        for defect in 0..5 {
            let f = if defect == 0 {
                ProducerFixture::new(sqlite, 1, false)
            } else {
                fixture(sqlite, 1, true)
            };
            let c = f.connect();
            let m = policy_message(
                1,
                if defect == 1 {
                    10
                } else if defect == 2 {
                    -2
                } else {
                    -1
                },
                if defect == 3 { 3 } else { 2 },
            );
            let before = f.rows();
            let e = f.effect(
                3,
                put(
                    c,
                    None,
                    m,
                    true,
                    if defect == 4 {
                        MqMqiUnitOfWork::Local {
                            unit: f.unit() + 100,
                        }
                    } else {
                        MqMqiUnitOfWork::NoSyncpoint
                    },
                ),
            );
            f.seed(&e);
            assert!(f.execute(&e).is_err());
            assert_eq!(f.rows(), before);
            assert_eq!(f.ports.gmt_calls.load(Ordering::SeqCst), 0);
            let guard = f.service.lock_selected().unwrap();
            let rich_state::StoredAuthority::Rich(s) = &*guard else {
                panic!()
            };
            assert_eq!(
                s.delivery
                    .depth(&crate::MqObjectName::new("Q").unwrap())
                    .unwrap(),
                0
            );
        }
    }
}

#[test]
fn defaults_late_catalog_control_source_clock_cancel_and_core_fences_abort_effective_policy() {
    for sqlite in [false, true] {
        for defect in 0..7 {
            let f = fixture(sqlite, 2, true);
            let c = f.connect();
            let unit = f.unit();
            let e = f.effect(
                3,
                put(
                    c,
                    None,
                    policy_message(2, -1, 2),
                    false,
                    MqMqiUnitOfWork::Local { unit },
                ),
            );
            f.seed(&e);
            let store = f.store.clone();
            let clock = f.clock.clone();
            let probe = f.inv.cancellation_probe.clone().unwrap();
            let ports = f.ports.clone();
            let key = e.idempotency_key.clone().unwrap();
            let snapshot = Arc::new(Mutex::new(None));
            let captured = snapshot.clone();
            *f.ports.after_capture.lock().unwrap() = Some(Box::new(move || {
                match defect {
                    0 | 1 => {
                        let (namespace, key) = if defect == 0 {
                            (CATALOG_NAMESPACE, CATALOG_KEY)
                        } else {
                            ("mq-state", "queues")
                        };
                        let mut row = store.get_provider_state(namespace, key).unwrap().unwrap();
                        let version = row.version;
                        row.version += 1;
                        store.put_provider_state(row, Some(version)).unwrap();
                    }
                    2 => probe.request(),
                    3 => clock.0.store(901, Ordering::SeqCst),
                    4 => {
                        store
                            .claim_stale_intent(&key, 8, "recovery", 900, 1, 10)
                            .unwrap();
                    }
                    5 => clock.0.store(7, Ordering::SeqCst),
                    _ => ports.mode.store(4, Ordering::SeqCst),
                }
                *captured.lock().unwrap() =
                    Some(store.list_provider_state_prefix("mq-", 4096).unwrap());
            }));
            let (live, audit) = {
                let guard = f.service.lock_selected().unwrap();
                let rich_state::StoredAuthority::Rich(s) = &*guard else {
                    panic!()
                };
                (
                    s.delivery.encode_live_checkpoint().unwrap(),
                    f.store.audit_records(&f.inv.execution_id, 0, 256).unwrap(),
                )
            };
            assert!(f.execute(&e).is_err(), "defect {defect}");
            assert_eq!(f.rows(), snapshot.lock().unwrap().clone().unwrap());
            let after = f.store.audit_records(&f.inv.execution_id, 0, 256).unwrap();
            if defect == 6 {
                // A source preparation failure publishes only its typed failure
                // audit under the original intent, never the delivery candidate.
                assert_eq!(&after[..audit.len()], audit);
                assert_eq!(after.len(), audit.len() + 1);
                assert_eq!(
                    after.last().unwrap().decision,
                    AuditDecision::ProviderFailure
                );
            } else {
                assert_eq!(after, audit);
            }
            let guard = f.service.lock_selected().unwrap();
            let rich_state::StoredAuthority::Rich(s) = &*guard else {
                panic!()
            };
            assert_eq!(s.delivery.encode_live_checkpoint().unwrap(), live);
            assert!(s.ownership.units[&unit].queues.is_empty());
        }
    }
}

#[test]
fn default_owned_sqlite_reopen_preserves_effective_payload_and_input_result_without_live_revival() {
    for persistent in [false, true] {
        let mut f = fixture(true, 2, persistent);
        let c = f.connect();
        let e = f.effect(
            3,
            put(
                c,
                None,
                policy_message(2, -1, 2),
                true,
                MqMqiUnitOfWork::NoSyncpoint,
            ),
        );
        f.seed(&e);
        let result = f.execute(&e).unwrap();
        assert_eq!(produced(&result).descriptor.fields().persistence, 2);
        let rows = f.rows();
        let db = f.db.take().unwrap();
        let (frame, inv, provider, saf, clock) = (
            f.frame,
            f.inv.clone(),
            f.provider.clone(),
            f.saf.clone(),
            f.clock.clone(),
        );
        drop(f);
        let store = db.open(256);
        assert_eq!(store.list_provider_state_prefix("mq-", 4096).unwrap(), rows);
        let service =
            MqService::open_selected_mqi(store.clone(), MqLimits::default(), 3, 5, saf, clock)
                .unwrap();
        assert!(
            service
                .execute_selected_mqi(
                    frame,
                    &inv,
                    e.mq_mqi_occurrence(HostLimits::default()).unwrap().unwrap(),
                    &provider,
                    HostLimits::default()
                )
                .is_err()
        );
        assert_eq!(store.list_provider_state_prefix("mq-", 4096).unwrap(), rows);
    }
}
