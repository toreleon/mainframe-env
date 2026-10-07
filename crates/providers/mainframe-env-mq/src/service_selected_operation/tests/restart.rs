use super::*;

static NEXT_FILE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
pub(super) struct Database(std::path::PathBuf);
impl Database {
    pub(super) fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "mq-selected-operation-{}-{}.sqlite",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::SeqCst)
        ));
        assert!(!path.exists());
        Self(path)
    }
    pub(super) fn open(&self) -> Arc<dyn PlatformStore> {
        Arc::new(
            SqliteStateStore::open(
                &format!("sqlite://{}?mode=rwc", self.0.display()),
                64 << 20,
                256,
            )
            .unwrap(),
        )
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let file = std::path::PathBuf::from(format!("{}{suffix}", self.0.display()));
            if file.exists() {
                std::fs::remove_file(file).unwrap();
            }
        }
    }
}

#[test]
fn sqlite_physical_reopen_preserves_pending_owners_receipts_and_uncompleted_intents() {
    let db = Database::new();
    let (inv, provider, old_c, old_frame, pending, reply, captured, audits) = {
        let f = Fixture::from_store(db.open());
        let c = f.connect();
        let o = f.open(c);
        let unit = f.unit();
        let e = f.effect(
            3,
            MqMqiRequest::Put {
                connection: c,
                object: o,
                put: put(MqMqiUnitOfWork::Local { unit }),
            },
        );
        f.seed(&e);
        let reply = f.execute(&e).unwrap();
        (
            f.inv.clone(),
            f.provider.clone(),
            c,
            f.frame,
            e,
            reply,
            f.rows(),
            f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap(),
        )
    };
    let store = db.open();
    assert_eq!(
        store.list_provider_state_prefix("mq-", 4096).unwrap(),
        captured
    );
    assert_eq!(
        store.audit_records(&inv.execution_id, 0, 128).unwrap(),
        audits
    );
    assert_eq!(
        store
            .effect(pending.idempotency_key.as_ref().unwrap())
            .unwrap()
            .unwrap()
            .state,
        EffectState::Intent
    );
    let clock = Arc::new(Clock(std::sync::atomic::AtomicU64::new(20)));
    let saf = Arc::new(Saf::default());
    let service =
        MqService::open_selected_mqi(store.clone(), MqLimits::default(), 3, 5, saf.clone(), clock)
            .unwrap();
    let process = service.mint_selected_process(&inv).unwrap();
    let (frame, owner) = service.bind_selected_root(process, &inv).unwrap();
    {
        let state = service.lock_selected().unwrap();
        let rich_state::StoredAuthority::Rich(s) = &*state else {
            panic!()
        };
        assert_eq!(s.runtime.as_ref().unwrap().control.registry_epoch, 2);
        assert!(s.runtime.as_ref().unwrap().connections.is_empty());
        assert_eq!(s.ownership.units[&1].state, ownership::UnitState::Pending);
        assert_eq!(s.delivery.unit_outcome(1), MqDeliveryOutcome::Pending);
        // Lossless stored observation is not an admission/handle permit.
        assert_eq!(
            s.receipts["effect-3"]
                .replay(HostLimits::default(), MqMqiLimits::default())
                .unwrap(),
            reply
        );
        let historical = s.receipts["effect-1"]
            .replay(HostLimits::default(), MqMqiLimits::default())
            .unwrap();
        let MqMqiOutput::Connected(historical) = output(historical) else {
            panic!()
        };
        assert!(historical.is_historical());
        assert_ne!(historical, old_c);
    }
    assert!(
        service
            .execute_selected_mqi(
                old_frame,
                &inv,
                pending
                    .mq_mqi_occurrence(HostLimits::default())
                    .unwrap()
                    .unwrap(),
                &provider,
                HostLimits::default()
            )
            .is_err()
    );
    assert_eq!(
        store.list_provider_state_prefix("mq-", 4096).unwrap(),
        captured
    );
    // A new cold incarnation is published atomically, never reset to epoch 1.
    let mut e = pending.clone();
    e.sequence = 4;
    e.idempotency_key =
        Some(IdempotencyKey::new("cold-connect", InvocationLimits::default()).unwrap());
    e.request = HostRequest::MqMqi(MqMqiHostRequest {
        mutation: Mutation {
            sequence: 4,
            idempotency_key: e.idempotency_key.clone().unwrap(),
            transaction: None,
        },
        envelope: MqMqiRequestEnvelope {
            context: MqMqiContext {
                owner,
                syncpoint_owner: MqSyncpointOwner::QueueManager,
            },
            limits: Default::default(),
            request: MqMqiRequest::Connect(MqMqiConnect {
                manager: None,
                sharing: MqHandleSharing::NonShared,
                options: MqMqiOptions::ContractDefault,
            }),
        },
    });
    let original = store
        .effect(pending.idempotency_key.as_ref().unwrap())
        .unwrap()
        .unwrap();
    let mut intent = original.clone();
    intent.key = e.idempotency_key.clone().unwrap();
    intent.sequence = 4;
    intent.request_digest = canonical_request_digest(&e.request).unwrap();
    intent.intent.audit_resource = Some(canonical_audit_resource_digest(&e.request));
    store.record_intent(intent).unwrap();
    let fresh = service
        .execute_selected_mqi(
            frame,
            &inv,
            e.mq_mqi_occurrence(HostLimits::default()).unwrap().unwrap(),
            &provider,
            HostLimits::default(),
        )
        .unwrap();
    let MqMqiOutput::Connected(new_c) = output(fresh) else {
        panic!()
    };
    assert_ne!(new_c, old_c);
    let state = service.lock_selected().unwrap();
    let rich_state::StoredAuthority::Rich(s) = &*state else {
        panic!()
    };
    assert_eq!(s.ownership.control.as_ref().unwrap().registry_epoch, 2);
    assert_eq!(s.ownership.units[&1].registry_epoch, 1);
    assert_eq!(s.delivery.unit_outcome(1), MqDeliveryOutcome::Pending);
    assert_eq!(store.effect(&original.key).unwrap(), Some(original));
}

#[test]
fn original_parented_batch_topology_is_not_forged_into_root_authority() {
    let f = Fixture::new(false);
    let mut child = f.inv.clone();
    child.parent_execution_id =
        Some(ExecutionId::new("parent", InvocationLimits::default()).unwrap());
    assert!(f.service.mint_selected_process(&child).is_err());
    let rows = f.rows();
    let effect = f.effect(
        1,
        MqMqiRequest::Connect(MqMqiConnect {
            manager: None,
            sharing: MqHandleSharing::NonShared,
            options: MqMqiOptions::ContractDefault,
        }),
    );
    f.seed(&effect);
    assert!(
        f.service
            .execute_selected_mqi(
                f.frame,
                &child,
                effect
                    .mq_mqi_occurrence(HostLimits::default())
                    .unwrap()
                    .unwrap(),
                &f.provider,
                HostLimits::default()
            )
            .is_err()
    );
    assert_eq!(f.rows(), rows);
    assert_eq!(f.saf.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn memory_sqlite_new_durable_incarnation_fences_old_runtime_handle_replay() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        f.connect();
        let old = f.effect(
            1,
            MqMqiRequest::Connect(MqMqiConnect {
                manager: None,
                sharing: MqHandleSharing::NonShared,
                options: MqMqiOptions::ContractDefault,
            }),
        );
        let cold = MqService::open_selected_mqi(
            f.store.clone(),
            MqLimits::default(),
            3,
            5,
            f.saf.clone(),
            f.clock.clone(),
        )
        .unwrap();
        let process = cold.mint_selected_process(&f.inv).unwrap();
        let (frame, owner) = cold.bind_selected_root(process, &f.inv).unwrap();
        let mut fresh = f.effect(
            3,
            MqMqiRequest::Connect(MqMqiConnect {
                manager: None,
                sharing: MqHandleSharing::NonShared,
                options: MqMqiOptions::ContractDefault,
            }),
        );
        let HostRequest::MqMqi(request) = &mut fresh.request else {
            panic!()
        };
        request.envelope.context.owner = owner;
        f.seed(&fresh);
        cold.execute_selected_mqi(
            frame,
            &f.inv,
            fresh
                .mq_mqi_occurrence(HostLimits::default())
                .unwrap()
                .unwrap(),
            &f.provider,
            HostLimits::default(),
        )
        .unwrap();
        let rows = f.rows();
        let saf = f.saf.calls.load(Ordering::SeqCst);
        let audits = f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap();
        assert_eq!(f.execute(&old), Err(HostProblem::UnknownOutcome));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.saf.calls.load(Ordering::SeqCst), saf);
        assert_eq!(
            f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap(),
            audits
        );
    }
}
