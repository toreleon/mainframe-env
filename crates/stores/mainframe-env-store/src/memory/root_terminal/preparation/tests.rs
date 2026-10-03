//! Test-only structural corruption; no production admission/intent relaxation.
use super::*;
use mainframe_env_execution_api::{ArtifactRef, IdempotencyKey, PrincipalId, RunUnitId, Selector};

fn prepared() -> (MemoryStore, RootPreparationPublication) {
    let bounds = InvocationLimits::default();
    let store = MemoryStore::new(StoreLimits::default());
    let execution = ExecutionRecord {
        execution_id: ExecutionId::new("preparation-root", bounds).unwrap(),
        run_unit_id: RunUnitId::new("preparation-run", bounds).unwrap(),
        principal: PrincipalId::new("IBMUSER", bounds).unwrap(),
        selector: Selector::new("program:ROOT", bounds).unwrap(),
        artifact: ArtifactRef::new(format!("sha256:{}", "a".repeat(64)), bounds).unwrap(),
        state: ExecutionState::Admitted,
        attempt: 1,
        version: 1,
        owner_lease: None,
        lease_expiry_tick: None,
        terminal_tick: None,
    };
    let event = LifecycleEvent {
        execution_id: execution.execution_id.clone(),
        run_unit_id: execution.run_unit_id.clone(),
        sequence: 1,
        attempt: 1,
        tick: 10,
        kind: mainframe_env_execution_api::LifecycleEventKind::Admitted,
    };
    let notification = OutboxRecord {
        notification_id: "preparation-root:00000000000000000001".into(),
        execution_id: execution.execution_id.clone(),
        sequence: 1,
        topic: "execution.lifecycle.v1".into(),
        payload: mainframe_env_execution_api::lifecycle_notification_payload(&event.kind),
        attempt: 0,
        delivered: false,
        delivered_tick: None,
        version: 1,
    };
    let claim = store
        .admit_root_driver(RootDriverAdmission {
            execution: execution.clone(),
            event,
            notification,
            invocation_key: IdempotencyKey::new("preparation-original", bounds).unwrap(),
            configuration_digest: [1; 32],
            deadline_tick: 100,
            provider_namespaces: vec!["preparation-owned".into()],
            provider_rows: vec![],
        })
        .unwrap();
    let request = RootPreparationPublication {
        claim,
        execution,
        anchor: mainframe_env_store_api::ProviderStateIdentity {
            namespace: "preparation-owned".into(),
            key: "first".into(),
        },
        observed_tick: 11,
        mutations: vec![ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: "preparation-owned".into(),
                key: "first".into(),
                version: 1,
                payload: b"owned".to_vec(),
            },
            expected_version: None,
        })],
    };
    (store, request)
}
fn unchanged(before: &State, after: &State) {
    assert_eq!(before.executions, after.executions);
    assert_eq!(before.events, after.events);
    assert_eq!(before.work, after.work);
    assert_eq!(before.checkpoints, after.checkpoints);
    assert_eq!(before.effects, after.effects);
    assert_eq!(before.audits, after.audits);
    assert_eq!(before.outbox, after.outbox);
    assert_eq!(before.provider_state, after.provider_state);
    assert_eq!(before.blob_bytes, after.blob_bytes);
    assert_eq!(before.provider_epoch, after.provider_epoch);
    assert_eq!(before.logical_tick, after.logical_tick);
    assert_eq!(before.next_audit_ordinal, after.next_audit_ordinal);
}
#[test]
fn memory_preparation_adversarial_original_effect_unknown_recovery_refuse() {
    for kind in 0..3 {
        let (store, request) = prepared();
        let key = IdempotencyKey::new("adversarial-effect", InvocationLimits::default()).unwrap();
        let effect = EffectRecord {
            execution_id: request.execution.execution_id.clone(),
            run_unit_id: request.execution.run_unit_id.clone(),
            sequence: 1,
            key: key.clone(),
            request_digest: [1; 32],
            digest_format: mainframe_env_store_api::EffectDigestFormat::CanonicalHostV1,
            state: if kind == 1 {
                mainframe_env_store_api::EffectState::UnknownOutcome
            } else {
                mainframe_env_store_api::EffectState::Intent
            },
            result_digest: None,
            resolved_tick: None,
            intent: mainframe_env_store_api::EffectIntentMetadata {
                owner: request.execution.execution_id.clone(),
                attempt: 1,
                capability: None,
                audit_resource: None,
                audit_invocation_key: None,
                created_tick: 10,
                recovery_after_tick: 100,
                epoch: 1,
                recovery_lease: (kind == 2).then(|| mainframe_env_store_api::EffectRecoveryLease {
                    owner: "recovery".into(),
                    attempt: 1,
                    epoch: 2,
                    expires_tick: 90,
                }),
            },
        };
        // Direct internal injection is a negative fixture, never an accepted coordinator state.
        store.lock().unwrap().effects.insert(key, effect);
        let before = store.snapshot(&store.lock().unwrap());
        assert!(store.mutate_root_preparation_states(request).is_err());
        unchanged(&before, &store.lock().unwrap());
    }
}
#[test]
fn memory_preparation_corrupt_indexes_event_and_epoch_roll_back() {
    for fault in 0..5 {
        let (store, mut request) = prepared();
        {
            let mut state = store.lock().unwrap();
            match fault {
                0 => {
                    state
                        .provider_state
                        .remove(&(RUN_NAMESPACE.into(), "preparation-run".into()));
                }
                1 => {
                    state
                        .provider_state
                        .get_mut(&(SCOPE_NAMESPACE.into(), "preparation-owned".into()))
                        .unwrap()
                        .payload = b"foreign".to_vec();
                }
                2 => {
                    state
                        .events
                        .get_mut(&request.execution.execution_id)
                        .unwrap()[0]
                        .tick = 9;
                }
                3 => {
                    state.provider_epoch = u64::MAX - 1;
                    request
                        .mutations
                        .push(ProviderStateMutation::Put(ProviderStateWrite {
                            record: ProviderStateRecord {
                                namespace: "preparation-owned".into(),
                                key: "second".into(),
                                version: 1,
                                payload: vec![1],
                            },
                            expected_version: None,
                        }));
                }
                _ => {
                    state.provider_state.insert(
                        (ACTOR_NAMESPACE.into(), "phantom".into()),
                        index(ACTOR_NAMESPACE, "phantom", "preparation-root"),
                    );
                }
            }
        }
        let before = store.snapshot(&store.lock().unwrap());
        assert!(
            store.mutate_root_preparation_states(request).is_err(),
            "fault {fault}"
        );
        unchanged(&before, &store.lock().unwrap());
    }
}

#[test]
fn memory_preparation_adversarial_work_and_checkpoint_refuse() {
    for work in [false, true] {
        let (store, request) = prepared();
        let e = &request.execution;
        if work {
            store.lock().unwrap().work.insert(
                "adversarial-work".into(),
                WorkRecord {
                    work_id: "adversarial-work".into(),
                    execution_id: e.execution_id.clone(),
                    required_selector: e.selector.clone(),
                    required_generation: "g1".into(),
                    artifact: e.artifact.clone(),
                    state: WorkState::Claimed,
                    priority: 0,
                    attempt: 1,
                    max_attempts: 1,
                    available_tick: 10,
                    deadline_tick: 100,
                    cancellation_requested: false,
                    worker_id: Some("worker".into()),
                    lease_id: Some("lease".into()),
                    lease_epoch: 1,
                    lease_expiry_tick: Some(90),
                    heartbeat_tick: Some(10),
                    terminal_tick: None,
                    checkpoint_id: None,
                    effect_sequence: 0,
                    payload: vec![1],
                },
            );
        } else {
            store.lock().unwrap().checkpoints.insert(
                e.execution_id.clone(),
                CheckpointRecord {
                    execution_id: e.execution_id.clone(),
                    run_unit_id: e.run_unit_id.clone(),
                    session_id: None,
                    schema_version: 1,
                    machine_schema_version: 1,
                    artifact: e.artifact.clone(),
                    provider_generation: "g1".into(),
                    required_host_interfaces: BTreeMap::new(),
                    effect_sequence: 0,
                    transaction: None,
                    principal: e.principal.clone(),
                    security_classification: "fixture".into(),
                    encryption_key_reference: None,
                    payload_size: 1,
                    payload_digest: [1; 32],
                    payload: vec![1],
                },
            );
        }
        let before = store.snapshot(&store.lock().unwrap());
        assert!(store.mutate_root_preparation_states(request).is_err());
        unchanged(&before, &store.lock().unwrap());
    }
}
