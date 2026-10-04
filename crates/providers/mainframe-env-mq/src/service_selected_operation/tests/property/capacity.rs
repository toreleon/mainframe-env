use super::*;

#[test]
fn memory_sqlite_actual_provider_row_and_payload_quotas_rollback_properties_receipts_and_audits() {
    for sqlite in [false, true] {
        for payload in [false, true] {
            let store: Arc<dyn PlatformStore> = if sqlite {
                Arc::new(
                    SqliteStateStore::open(
                        "sqlite::memory:",
                        if payload { 16384 } else { 64 << 20 },
                        256,
                    )
                    .unwrap(),
                )
            } else {
                Arc::new(MemoryStore::new(mainframe_env_store::StoreLimits {
                    max_audits: 256,
                    max_provider_state: 256,
                    max_blob_bytes: if payload { 16384 } else { 64 << 20 },
                    ..Default::default()
                }))
            };
            let f = Fixture::from_store(store);
            let c = f.connect();
            let h = hmsg(f.call(2, create(c)));
            let expected = if payload {
                vec![255; 8000]
            } else {
                vec![0, 255]
            };
            f.call(
                3,
                set(c, h, "a", MqPropertyType::ByteString, expected.clone()),
            );
            let request = if payload {
                inquire(c, h, "a", 64, 8000)
            } else {
                create(c)
            };
            let e = f.effect(4, request);
            f.seed(&e);
            if !payload {
                let count = f.rows().len()
                    + f.store
                        .list_provider_state_prefix("durable-", 4096)
                        .unwrap()
                        .len();
                for n in count..256 {
                    f.store
                        .put_provider_state(
                            ProviderStateRecord {
                                namespace: "property-quota".into(),
                                key: format!("{n}"),
                                version: 1,
                                payload: vec![1],
                            },
                            None,
                        )
                        .unwrap();
                }
            }
            let rows = f.rows();
            let state = snapshot(&f);
            let audits = f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap();
            assert!(f.execute(&e).is_err(), "sqlite={sqlite} payload={payload}");
            assert_eq!(f.rows(), rows);
            assert_eq!(snapshot(&f), state);
            assert_eq!(
                f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap(),
                audits
            );
            assert!(
                f.store
                    .get_provider_state(receipt::NAMESPACE, "effect-4")
                    .unwrap()
                    .is_none()
            );
            let MqMqiOutput::PropertyObservation(MqPropertyObservation::Inquired(v)) =
                peek(&f, inquire(c, h, "a", 64, 8000))
            else {
                panic!()
            };
            assert_eq!(v.copied_value, expected);
        }
    }
}

#[test]
fn finite_limits_wrong_profile_hierarchy_and_unreviewed_options_cannot_mutate() {
    let f = Fixture::new(false);
    let c = f.connect();
    let h = hmsg(f.call(2, create(c)));
    f.call(3, set(c, h, "a.b", MqPropertyType::Null, vec![]));
    let state = snapshot(&f);
    let rows = f.rows();
    let e = f.effect(4, set(c, h, "a", MqPropertyType::Null, vec![]));
    f.seed(&e);
    assert_eq!(f.execute(&e), Err(HostProblem::Unsupported));
    assert_eq!(snapshot(&f), state);
    assert_eq!(f.rows(), rows);
    let mut e = f.effect(
        5,
        set(c, h, "large", MqPropertyType::ByteString, vec![1; 8]),
    );
    let HostRequest::MqMqi(v) = &mut e.request else {
        panic!()
    };
    v.envelope.limits.message.property_value_bytes = 4;
    assert!(e.mq_mqi_occurrence(HostLimits::default()).is_err());
    assert_eq!(snapshot(&f), state);
    for environment in [
        MqHostEnvironment::ZosCics,
        MqHostEnvironment::ZosIms,
        MqHostEnvironment::OtherBindings,
    ] {
        let mut e = f.effect(6, create(c));
        let HostRequest::MqMqi(v) = &mut e.request else {
            panic!()
        };
        v.envelope.context.owner.environment = environment;
        assert!(e.mq_mqi_occurrence(HostLimits::default()).is_err());
        assert_eq!(snapshot(&f), state);
    }
    for (call, bits) in [
        (MqMqiCall::CreateMessageHandle, 2),
        (MqMqiCall::SetProperty, 4),
        (MqMqiCall::InquireProperty, 32),
        (MqMqiCall::DeleteProperty, 1),
        (MqMqiCall::DeleteMessageHandle, 1),
    ] {
        assert!(MqPropertyOptions::checked(call, 1, bits).is_err());
        assert_eq!(snapshot(&f), state);
    }
    assert_eq!(f.rows(), rows);
}
