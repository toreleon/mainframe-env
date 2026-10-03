use super::*;
#[test]
fn memory_owned_sqlite_late_cas_controls_core_and_source_fail_without_adoption() {
    for sqlite in [false, true] {
        for case in 0..6 {
            let f = ProducerFixture::new(sqlite, 2, false);
            let c = f.connect();
            let unit = f.unit();
            let e = f.effect(
                3,
                put(
                    c,
                    None,
                    message(2, false, 77),
                    false,
                    MqMqiUnitOfWork::Local { unit },
                ),
            );
            f.seed(&e);
            let store = f.store.clone();
            let clock = f.clock.clone();
            let probe = f.inv.cancellation_probe.clone().unwrap();
            let key = e.idempotency_key.clone().unwrap();
            let snapshot = Arc::new(Mutex::new(None));
            let captured = snapshot.clone();
            *f.ports.after_capture.lock().unwrap() = Some(Box::new(move || {
                match case {
                    0 | 1 => {
                        let namespace = if case == 0 {
                            CATALOG_NAMESPACE
                        } else {
                            "mq-state"
                        };
                        let rowkey = if case == 0 { CATALOG_KEY } else { "queues" };
                        let mut record = store
                            .get_provider_state(namespace, rowkey)
                            .unwrap()
                            .unwrap();
                        let old = record.version;
                        record.version += 1;
                        store.put_provider_state(record, Some(old)).unwrap();
                    }
                    2 => probe.request(),
                    3 => clock.0.store(901, Ordering::SeqCst),
                    4 => {
                        store
                            .claim_stale_intent(&key, 8, "recovery", 900, 1, 10)
                            .unwrap();
                    }
                    _ => clock.0.store(7, Ordering::SeqCst),
                }
                *captured.lock().unwrap() =
                    Some(store.list_provider_state_prefix("mq-", 4096).unwrap());
            }));
            let live = {
                let guard = f.service.lock_selected().unwrap();
                let rich_state::StoredAuthority::Rich(s) = &*guard else {
                    panic!()
                };
                s.delivery.encode_live_checkpoint().unwrap()
            };
            let audits = f.store.audit_records(&f.inv.execution_id, 0, 256).unwrap();
            assert!(f.execute(&e).is_err(), "case {case}");
            assert_eq!(f.rows(), snapshot.lock().unwrap().clone().unwrap());
            let guard = f.service.lock_selected().unwrap();
            let rich_state::StoredAuthority::Rich(s) = &*guard else {
                panic!()
            };
            assert_eq!(s.delivery.encode_live_checkpoint().unwrap(), live);
            assert!(s.ownership.units[&unit].queues.is_empty());
            assert_eq!(
                f.store.audit_records(&f.inv.execution_id, 0, 256).unwrap(),
                audits
            );
            assert_eq!(f.ports.gmt_calls.load(Ordering::SeqCst), 1);
        }
    }
}
#[test]
fn zero_queue_max_empty_body_nonpersistent_duplicates_and_changed_replay_identity() {
    for sqlite in [false, true] {
        let f = ProducerFixture::limited(sqlite, 1, false, 256, 0);
        let c = f.connect();
        let o = f.open(c);
        let mut m = message(1, false, i32::MAX);
        m.body.clear();
        match &mut m.descriptor {
            MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => fields.persistence = 0,
        };
        let e = f.effect(
            3,
            put(c, Some(o), m.clone(), true, MqMqiUnitOfWork::NoSyncpoint),
        );
        f.seed(&e);
        let first = f.execute(&e).unwrap();
        f.call(
            4,
            put(c, None, m.clone(), true, MqMqiUnitOfWork::NoSyncpoint),
        );
        assert_eq!(f.get(c, o, 5, 1, false).unwrap().body, Vec::<u8>::new());
        assert_eq!(
            f.get(c, o, 6, 1, false)
                .unwrap()
                .descriptor
                .fields()
                .persistence,
            0
        );
        assert_eq!(f.execute(&e).unwrap(), first);
        let rows = f.rows();
        let mut changed = e.clone();
        if let HostRequest::MqMqi(h) = &mut changed.request {
            if let MqMqiRequest::FullPut { put, .. } = &mut h.envelope.request {
                put.message.body.push(1);
            }
        }
        assert!(f.execute(&changed).is_err());
        assert_eq!(f.rows(), rows);
        let mut foreign = f.inv.clone();
        foreign.deadline_tick += 1;
        assert!(
            f.service
                .execute_selected_mqi(
                    f.frame,
                    &foreign,
                    e.mq_mqi_occurrence(HostLimits::default()).unwrap().unwrap(),
                    &f.provider,
                    HostLimits::default()
                )
                .is_err()
        );
        assert_eq!(f.rows(), rows);
        assert_eq!(f.ports.gmt_calls.load(Ordering::SeqCst), 0);
    }
}
