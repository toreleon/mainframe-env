use super::*;

#[test]
fn source_errors_invalid_inputs_catalog_lengths_and_saf_refuse_without_payload_adoption() {
    for sqlite in [false, true] {
        for mode in [1, 2, 3, 4, 5] {
            let f = ProducerFixture::new(sqlite, 2, false);
            let c = f.connect();
            f.ports.mode.store(mode, Ordering::SeqCst);
            let before = f.rows();
            let e = f.effect(
                3,
                put(
                    c,
                    None,
                    message(2, false, -1),
                    false,
                    MqMqiUnitOfWork::NoSyncpoint,
                ),
            );
            f.seed(&e);
            assert!(f.execute(&e).is_err());
            assert_eq!(f.rows(), before);
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
            assert!(s.ownership.units.values().all(|u| u.queues.is_empty()));
        }
        for variant in 0..9 {
            let f = ProducerFixture::limited(sqlite, 2, false, 256, 4);
            let c = f.connect();
            let mut m = message(2, false, -1);
            m.body.truncate(4);
            let fields = match &mut m.descriptor {
                MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => fields,
            };
            match variant {
                0 => fields.msg_id = [0; 24],
                1 => fields.priority = 1,
                2 => fields.persistence = 2,
                3 => fields.expiry = 1,
                4 => fields.report = 1,
                5 => fields.coded_char_set_id = 0,
                6 => fields.format = *b"MQHRF2  ",
                7 => m.body.push(1),
                _ => m.properties.push(MqMessageProperty {
                    name: "x".into(),
                    kind: MqPropertyType::ByteString,
                    value: vec![1],
                }),
            }
            let rows = f.rows();
            let e = f.effect(3, put(c, None, m, false, MqMqiUnitOfWork::NoSyncpoint));
            f.seed(&e);
            assert!(f.execute(&e).is_err());
            assert_eq!(f.rows(), rows);
            assert_eq!(f.ports.gmt_calls.load(Ordering::SeqCst), 0);
        }
        let f = ProducerFixture::new(sqlite, 1, true);
        let c = f.connect();
        let rows = f.rows();
        f.saf.deny.store(true, Ordering::SeqCst);
        let e = f.effect(
            3,
            put(
                c,
                None,
                message(1, true, 0),
                false,
                MqMqiUnitOfWork::NoSyncpoint,
            ),
        );
        f.seed(&e);
        assert_eq!(f.execute(&e), Err(HostProblem::Unauthorized));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.ports.context_calls.load(Ordering::SeqCst), 0);
    }
}
#[test]
fn synchronous_reentry_refused_and_unavailable_optional_context_is_distinct_from_error() {
    for sqlite in [false, true] {
        let f = ProducerFixture::new(sqlite, 2, true);
        let c = f.connect();
        *f.ports.reentrant.lock().unwrap() = Some(Arc::downgrade(&f.service));
        f.ports.mode.store(6, Ordering::SeqCst);
        let p = f.call(
            3,
            put(
                c,
                None,
                message(2, true, -1),
                false,
                MqMqiUnitOfWork::NoSyncpoint,
            ),
        );
        assert_eq!(produced(&p).descriptor.fields().user_identifier, [0x40; 12]);
        assert_eq!(produced(&p).descriptor.fields().accounting_token, [0; 32]);
        assert_eq!(f.ports.gmt_calls.load(Ordering::SeqCst), 1);
    }
}
#[test]
fn cancellation_late_saf_and_audit_saturation_roll_back_full_put() {
    for sqlite in [false, true] {
        let f = ProducerFixture::new(sqlite, 2, false);
        let c = f.connect();
        let rows = f.rows();
        let probe = f.inv.cancellation_probe.clone().unwrap();
        *f.saf.hook.lock().unwrap() = Some(Box::new(move || probe.request()));
        let e = f.effect(
            3,
            put(
                c,
                None,
                message(2, false, 0),
                false,
                MqMqiUnitOfWork::NoSyncpoint,
            ),
        );
        f.seed(&e);
        assert_eq!(f.execute(&e), Err(HostProblem::Cancelled));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.ports.gmt_calls.load(Ordering::SeqCst), 0);
        let f = ProducerFixture::new(sqlite, 2, false);
        let c = f.connect();
        let e = f.effect(
            3,
            put(
                c,
                None,
                message(2, false, 0),
                false,
                MqMqiUnitOfWork::NoSyncpoint,
            ),
        );
        f.seed(&e);
        // Intent precedes provider dispatch/quota exhaustion. Memory audit
        // quota / SQLite shared audit-provider row quota are real physical caps.
        super::super::bounds::fill_audits(&f.f);
        let rows = f.rows();
        assert!(f.execute(&e).is_err());
        assert_eq!(f.rows(), rows);
        assert!(f.ports.gmt_calls.load(Ordering::SeqCst) <= 1);
    }
}
