use super::*;

fn fill_audits(f: &Fixture) {
    let e = f.effect(
        200,
        MqMqiRequest::Connect(MqMqiConnect {
            manager: None,
            sharing: MqHandleSharing::NonShared,
            options: MqMqiOptions::ContractDefault,
        }),
    );
    let mut n = f
        .store
        .audit_records(&f.inv.execution_id, 0, 256)
        .unwrap()
        .len();
    while n < 256 {
        let appended = f.store.record_audit(AuditRecord {
            execution_id: f.inv.execution_id.clone(),
            run_unit_id: f.inv.run_unit_id.clone(),
            attempt: 1,
            effect_sequence: 1000 + n as u64,
            observed_tick: 20,
            principal: f.inv.principal.id().clone(),
            invocation_key: IdempotencyKey::new(
                format!("quota-audit-{n}"),
                InvocationLimits::default(),
            )
            .unwrap(),
            capability: f.provider.capability.clone(),
            resource: canonical_audit_resource_digest(&e.request),
            decision: AuditDecision::Success,
        });
        match appended {
            Ok(()) => {}
            Err(StoreError::CapacityExceeded) => break,
            Err(error) => panic!("audit quota fixture: {error:?}"),
        }
        n += 1;
    }
}

#[test]
fn memory_sqlite_late_publication_quota_rolls_back_rows_owner_epoch_and_pending_commit() {
    for sqlite in [false, true] {
        for pending in [false, true] {
            let f = Fixture::new(sqlite);
            let e = if pending {
                let c = f.connect();
                let o = f.open(c);
                let unit = f.unit();
                f.call(
                    3,
                    MqMqiRequest::Put {
                        connection: c,
                        object: o,
                        put: put(MqMqiUnitOfWork::Local { unit }),
                    },
                );
                f.effect(
                    4,
                    MqMqiRequest::Commit {
                        connection: c,
                        unit,
                    },
                )
            } else {
                f.effect(
                    1,
                    MqMqiRequest::Connect(MqMqiConnect {
                        manager: None,
                        sharing: MqHandleSharing::NonShared,
                        options: MqMqiOptions::ContractDefault,
                    }),
                )
            };
            f.seed(&e);
            fill_audits(&f);
            let before = f.rows();
            let audits = f.store.audit_records(&f.inv.execution_id, 0, 256).unwrap();
            assert!(f.execute(&e).is_err());
            assert_eq!(f.rows(), before);
            assert_eq!(
                f.store.audit_records(&f.inv.execution_id, 0, 256).unwrap(),
                audits
            );
            let state = f.service.lock_selected().unwrap();
            let rich_state::StoredAuthority::Rich(s) = &*state else {
                panic!()
            };
            if pending {
                assert_eq!(s.delivery.unit_outcome(1), MqDeliveryOutcome::Pending);
                assert_eq!(s.ownership.units[&1].state, ownership::UnitState::Pending);
                assert_eq!(s.runtime.as_ref().unwrap().connections[0].unit, 1);
            } else {
                assert!(s.ownership.control.is_none());
                assert!(s.runtime.as_ref().unwrap().connections.is_empty());
            }
            assert_eq!(
                f.store
                    .effect(e.idempotency_key.as_ref().unwrap())
                    .unwrap()
                    .unwrap()
                    .state,
                EffectState::Intent
            );
        }
    }
}

#[test]
fn selected_reader_and_plan_reject_unmodeled_namespace_bad_owner_and_receipt_profiles() {
    let f = Fixture::new(false);
    f.connect();
    let captured = f.rows();
    for case in 0..6 {
        let mut records = captured.clone();
        match case {
            0 => records.push(ProviderStateRecord {
                namespace: "mq-selected-v1-unmodeled".into(),
                key: "x".into(),
                version: 1,
                payload: b"{}".to_vec(),
            }),
            1 => {
                let row = records
                    .iter_mut()
                    .find(|r| r.namespace == ownership::UOW_NAMESPACE)
                    .unwrap();
                let mut value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
                value["value"]["coordinator"] = serde_json::json!("caller-selected");
                row.payload = serde_json::to_vec(&value).unwrap();
            }
            2 => {
                let row = records
                    .iter_mut()
                    .find(|r| r.namespace == receipt::NAMESPACE)
                    .unwrap();
                let mut value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
                value["value"]["reply"]["profiles"]["host"][0] = serde_json::json!(usize::MAX);
                row.payload = serde_json::to_vec(&value).unwrap();
            }
            3 => {
                let row = records
                    .iter_mut()
                    .find(|r| r.namespace == ownership::CONTROL_NAMESPACE)
                    .unwrap();
                let mut value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
                value["value"]["registry_epoch"] = serde_json::json!(0);
                row.payload = serde_json::to_vec(&value).unwrap();
            }
            4 => records.retain(|r| r.namespace != ownership::UOW_NAMESPACE),
            _ => {
                let row = records
                    .iter_mut()
                    .find(|r| r.namespace == ownership::CONTROL_NAMESPACE)
                    .unwrap();
                let mut value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
                value["value"]["next_unit"] = serde_json::json!(i64::MAX as u64);
                row.payload = serde_json::to_vec(&value).unwrap();
            }
        }
        assert!(
            rich_state::decode_records(records, 3, 5, rich_state::ReaderLimits::default()).is_err()
        );
    }
    let state = f.service.lock_selected().unwrap();
    let rich_state::StoredAuthority::Rich(s) = &*state else {
        panic!()
    };
    let invalid = ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: "mq-other-authority".into(),
            key: "x".into(),
            version: 1,
            payload: b"{}".to_vec(),
        },
        expected_version: None,
    });
    assert!(
        s.plan_selected_delivery(&s.delivery, vec![invalid], Default::default())
            .is_err()
    );
    let control = s.ownership.control.as_ref().unwrap().clone();
    let mut missing = s.ownership.units.clone();
    missing.clear();
    assert!(
        s.ownership
            .changes(&control, &missing, MqLimits::default())
            .is_err()
    );
    assert_eq!(f.rows(), captured);
}
