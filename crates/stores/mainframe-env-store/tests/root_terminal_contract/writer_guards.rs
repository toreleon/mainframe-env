//! Store guard proof only: not compiled, installed, MQ or SAF acceptance.
use super::*;
use sha2::{Digest, Sha256};

fn checkpoint(execution: &ExecutionRecord) -> CheckpointRecord {
    let payload = b"legacy-checkpoint-payload".to_vec();
    CheckpointRecord {
        execution_id: execution.execution_id.clone(),
        run_unit_id: execution.run_unit_id.clone(),
        session_id: None,
        schema_version: 1,
        machine_schema_version: 1,
        artifact: execution.artifact.clone(),
        provider_generation: "test-generation".into(),
        required_host_interfaces: Default::default(),
        effect_sequence: 0,
        transaction: None,
        principal: execution.principal.clone(),
        security_classification: "test".into(),
        encryption_key_reference: None,
        payload_size: payload.len() as u64,
        payload_digest: Sha256::digest(&payload).into(),
        payload,
    }
}

fn direct_checkpoint_refuses(store: &dyn PlatformStore) {
    let (claim, execution) = running(store);
    let epoch = store.provider_state_retention_epoch().unwrap();
    assert_eq!(
        store.put_checkpoint(checkpoint(&execution)),
        Err(StoreError::InvalidTransition)
    );
    assert!(
        store
            .get_checkpoint(&execution.execution_id)
            .unwrap()
            .is_none()
    );
    assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
    // A refused transfer must leave this same finite root closable.
    store.close_root_driver(&claim, &execution, 10).unwrap();
}

fn terminal_events() -> [LifecycleEventKind; 6] {
    [
        LifecycleEventKind::Completing,
        LifecycleEventKind::Completed { return_code: 0 },
        LifecycleEventKind::Abend,
        LifecycleEventKind::Failed,
        LifecycleEventKind::Cancelled,
        LifecycleEventKind::TimedOut,
    ]
}

fn direct_root_event_refuses(store: &dyn PlatformStore) {
    let (claim, execution) = running(store);
    let epoch = store.provider_state_retention_epoch().unwrap();
    let events = store.events(&execution.execution_id, 1, 32).unwrap();
    for kind in terminal_events() {
        assert_eq!(
            store.append_event(event(&execution, 4, kind)),
            Err(StoreError::InvalidTransition)
        );
        assert_eq!(
            store.events(&execution.execution_id, 1, 32).unwrap(),
            events
        );
        assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
        assert_eq!(
            store.get_execution(&execution.execution_id).unwrap(),
            Some(execution.clone())
        );
    }
    store.close_root_driver(&claim, &execution, 10).unwrap();
}

fn journal_root_event_refuses(store: &dyn PlatformStore) {
    let (claim, execution) = running(store);
    let epoch = store.provider_state_retention_epoch().unwrap();
    let events = store.events(&execution.execution_id, 1, 32).unwrap();
    let notifications = store.pending_notifications(32).unwrap();
    for kind in terminal_events() {
        let event = event(&execution, 4, kind);
        assert_eq!(
            store.commit_execution_step(
                &execution.execution_id,
                execution.version,
                None,
                event.clone(),
                None,
                None,
                None,
                outbox(&event)
            ),
            Err(StoreError::InvalidTransition)
        );
        assert_eq!(
            store.events(&execution.execution_id, 1, 32).unwrap(),
            events
        );
        assert_eq!(store.pending_notifications(32).unwrap(), notifications);
        assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
        assert_eq!(
            store.get_execution(&execution.execution_id).unwrap(),
            Some(execution.clone())
        );
    }
    store.close_root_driver(&claim, &execution, 10).unwrap();
}

fn journal_checkpoint_refuses(store: &dyn PlatformStore) {
    let (claim, execution) = running(store);
    let event = event(&execution, 4, LifecycleEventKind::Suspended);
    let epoch = store.provider_state_retention_epoch().unwrap();
    let events = store.events(&execution.execution_id, 1, 32).unwrap();
    let notifications = store.pending_notifications(32).unwrap();
    assert_eq!(
        store.commit_execution_step(
            &execution.execution_id,
            execution.version,
            Some(ExecutionState::Suspended),
            event.clone(),
            None,
            None,
            Some(checkpoint(&execution)),
            outbox(&event)
        ),
        Err(StoreError::InvalidTransition)
    );
    assert!(
        store
            .get_checkpoint(&execution.execution_id)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store.get_execution(&execution.execution_id).unwrap(),
        Some(execution.clone())
    );
    assert_eq!(
        store.events(&execution.execution_id, 1, 32).unwrap(),
        events
    );
    assert_eq!(store.pending_notifications(32).unwrap(), notifications);
    assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
    store.close_root_driver(&claim, &execution, 10).unwrap();
}

macro_rules! adapter_tests {
    ($memory:ident, $sqlite:ident, $proof:ident) => {
        #[test]
        fn $memory() {
            $proof(&MemoryStore::new(StoreLimits::default()));
        }
        #[test]
        fn $sqlite() {
            let fixture = OwnedSqlite::new();
            $proof(fixture.store());
        }
    };
}
adapter_tests!(
    memory_direct_checkpoint,
    sqlite_direct_checkpoint,
    direct_checkpoint_refuses
);
adapter_tests!(
    memory_direct_root_event,
    sqlite_direct_root_event,
    direct_root_event_refuses
);
adapter_tests!(
    memory_journal_root_event,
    sqlite_journal_root_event,
    journal_root_event_refuses
);
adapter_tests!(
    memory_journal_checkpoint,
    sqlite_journal_checkpoint,
    journal_checkpoint_refuses
);

fn uncertain_outbox_refuses(store: &dyn PlatformStore) {
    let (claim, execution) = running(store);
    store.fence_root_driver(&claim, &execution, 10).unwrap();
    let before = store.pending_notifications(32).unwrap();
    let epoch = store.provider_state_retention_epoch().unwrap();
    let next = event(&execution, 4, LifecycleEventKind::Started);
    assert_eq!(
        store.append_notification(outbox(&next)),
        Err(StoreError::InvalidTransition)
    );
    assert_eq!(store.pending_notifications(32).unwrap(), before);
    assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
}

fn uncertain_audited_publication_refuses(store: &dyn PlatformStore) {
    let (claim, execution) = running(store);
    // Attributed physical intent fixture only. These hashes are not an
    // installed host occurrence or native core finality/settlement evidence.
    let mut intent = orphan_intent();
    intent.execution_id = execution.execution_id.clone();
    intent.intent.owner = execution.execution_id.clone();
    store.record_intent(intent.clone()).unwrap();
    store.fence_root_driver(&claim, &execution, 10).unwrap();
    let audit = AuditRecord {
        execution_id: execution.execution_id.clone(),
        run_unit_id: execution.run_unit_id.clone(),
        principal: execution.principal.clone(),
        attempt: execution.attempt,
        effect_sequence: intent.sequence,
        invocation_key: intent.intent.audit_invocation_key.clone().unwrap(),
        capability: intent.intent.capability.clone().unwrap(),
        resource: intent.intent.audit_resource.unwrap(),
        decision: AuditDecision::Success,
        observed_tick: 10,
    };
    let epoch = store.provider_state_retention_epoch().unwrap();
    for mutations in [
        Vec::new(),
        vec![ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: "unscoped-test".into(),
                key: "audit-bypass".into(),
                version: 1,
                payload: vec![1],
            },
            expected_version: None,
        })],
    ] {
        assert_eq!(
            store.publish_provider_states_audited(AuditedProviderPublication {
                intent: intent.clone(),
                audit: audit.clone(),
                observed_tick: 10,
                mutations,
            }),
            Err(StoreError::InvalidTransition)
        );
        assert!(
            store
                .audit_subject_records(&execution.execution_id, 32)
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .get_provider_state("unscoped-test", "audit-bypass")
                .unwrap()
                .is_none()
        );
        assert_eq!(store.effect(&intent.key).unwrap(), Some(intent.clone()));
        assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
    }
}
adapter_tests!(
    memory_uncertain_outbox,
    sqlite_uncertain_outbox,
    uncertain_outbox_refuses
);
adapter_tests!(
    memory_uncertain_audited,
    sqlite_uncertain_audited,
    uncertain_audited_publication_refuses
);

fn child(store: &dyn PlatformStore) -> ExecutionRecord {
    let (claim, parent) = running(store);
    // Physical enrollment fixture only, with the actual stored parent intent,
    // CALL dependency and catalog read back by the existing admission helper.
    // No compiled program/host attestation or application finality is claimed.
    let mut intent = orphan_intent();
    intent.execution_id = parent.execution_id.clone();
    intent.intent.owner = parent.execution_id.clone();
    store.record_intent(intent.clone()).unwrap();
    let mut execution = admission().execution;
    execution.execution_id = ExecutionId::new("native-child", InvocationLimits::default()).unwrap();
    execution.selector = Selector::new("program:CHILD", InvocationLimits::default()).unwrap();
    let catalog = ProviderStateRecord {
        namespace: "batch-program".into(),
        key: "CHILD".into(),
        version: 1,
        payload: execution.artifact.as_str().as_bytes().to_vec(),
    };
    let call = ProviderStateRecord {
        namespace: "native-root-owned-test".into(),
        key: "original-call".into(),
        version: 1,
        payload: b"physical-call-fixture".to_vec(),
    };
    let mut occurrence = RootProviderRowAdmission {
        claim: claim.clone(),
        execution: parent.clone(),
        effect_key: intent.key,
        effect_sequence: intent.sequence,
        request_digest: intent.request_digest,
        identity: ProviderStateIdentity {
            namespace: call.namespace.clone(),
            key: call.key.clone(),
        },
        observed_tick: 10,
    };
    store
        .register_root_provider_row(occurrence.clone())
        .unwrap();
    occurrence.identity = ProviderStateIdentity {
        namespace: catalog.namespace.clone(),
        key: catalog.key.clone(),
    };
    store
        .register_root_provider_row(occurrence.clone())
        .unwrap();
    store.put_provider_state(catalog.clone(), None).unwrap();
    store.put_provider_state(call.clone(), None).unwrap();
    occurrence.identity = ProviderStateIdentity {
        namespace: call.namespace.clone(),
        key: call.key.clone(),
    };
    let admitted = event(&execution, 1, LifecycleEventKind::Admitted);
    store
        .admit_root_child(RootChildAdmission {
            claim,
            parent: parent.execution_id,
            parent_occurrence: occurrence,
            execution: execution.clone(),
            event: admitted.clone(),
            notification: outbox(&admitted),
            call,
            catalog,
        })
        .unwrap();
    for (sequence, state, kind) in [
        (2, ExecutionState::Queued, LifecycleEventKind::Queued),
        (3, ExecutionState::Running, LifecycleEventKind::Started),
    ] {
        let event = event(&execution, sequence, kind);
        execution = store
            .commit_execution_step(
                &execution.execution_id,
                execution.version,
                Some(state),
                event.clone(),
                None,
                None,
                None,
                outbox(&event),
            )
            .unwrap();
    }
    execution
}

fn child_completion_remains_valid(store: &dyn PlatformStore, abend: bool) {
    let mut execution = child(store);
    assert_eq!(
        store.put_checkpoint(checkpoint(&execution)),
        Err(StoreError::InvalidTransition)
    );
    for (state, kind) in if abend {
        vec![(ExecutionState::Failed, LifecycleEventKind::Abend)]
    } else {
        vec![
            (ExecutionState::Completing, LifecycleEventKind::Completing),
            (
                ExecutionState::Completed,
                LifecycleEventKind::Completed { return_code: 4 },
            ),
        ]
    } {
        let event = event(&execution, execution.version + 1, kind);
        execution = store
            .commit_execution_step(
                &execution.execution_id,
                execution.version,
                Some(state),
                event.clone(),
                None,
                None,
                None,
                outbox(&event),
            )
            .unwrap();
    }
    assert_eq!(
        execution.state,
        if abend {
            ExecutionState::Failed
        } else {
            ExecutionState::Completed
        }
    );
    assert_eq!(
        store
            .get_execution(&admission().execution.execution_id)
            .unwrap()
            .unwrap()
            .state,
        ExecutionState::Running
    );
    assert_eq!(
        store.put_checkpoint(checkpoint(&execution)),
        Err(StoreError::InvalidTransition)
    );
}

fn legacy_checkpoint_and_events_remain_valid(store: &dyn PlatformStore) {
    let _ = running(store);
    let mut execution = admission().execution;
    execution.execution_id = ExecutionId::new("legacy-actor", InvocationLimits::default()).unwrap();
    execution.run_unit_id = RunUnitId::new("legacy-run", InvocationLimits::default()).unwrap();
    let admitted = event(&execution, 1, LifecycleEventKind::Admitted);
    store
        .admit_execution(execution.clone(), admitted.clone(), outbox(&admitted))
        .unwrap();
    for (sequence, state, kind) in [
        (2, ExecutionState::Queued, LifecycleEventKind::Queued),
        (3, ExecutionState::Running, LifecycleEventKind::Started),
    ] {
        let event = event(&execution, sequence, kind);
        execution = store
            .commit_execution_step(
                &execution.execution_id,
                execution.version,
                Some(state),
                event.clone(),
                None,
                None,
                None,
                outbox(&event),
            )
            .unwrap();
    }
    let original = checkpoint(&execution);
    store.put_checkpoint(original.clone()).unwrap();
    assert_eq!(
        store.get_checkpoint(&execution.execution_id).unwrap(),
        Some(original.clone())
    );
    store.delete_checkpoint(&execution.execution_id).unwrap();
    let suspended = event(&execution, 4, LifecycleEventKind::Suspended);
    let result = store
        .commit_execution_step(
            &execution.execution_id,
            execution.version,
            Some(ExecutionState::Suspended),
            suspended.clone(),
            None,
            None,
            Some(original.clone()),
            outbox(&suspended),
        )
        .unwrap();
    assert_eq!(result.state, ExecutionState::Suspended);
    assert_eq!(
        store.get_checkpoint(&execution.execution_id).unwrap(),
        Some(original)
    );
    // Preserve the legacy EventStore contract; this does not settle the root.
    store
        .append_event(event(&execution, 5, LifecycleEventKind::Abend))
        .unwrap();
}

fn work(execution: &ExecutionRecord) -> WorkRecord {
    WorkRecord {
        work_id: "unsupported-native-work".into(),
        execution_id: execution.execution_id.clone(),
        required_selector: execution.selector.clone(),
        required_generation: "test-generation".into(),
        artifact: execution.artifact.clone(),
        state: WorkState::Queued,
        priority: 0,
        attempt: 0,
        max_attempts: 1,
        available_tick: 10,
        deadline_tick: 100,
        cancellation_requested: false,
        worker_id: None,
        lease_id: None,
        lease_epoch: 0,
        lease_expiry_tick: None,
        heartbeat_tick: None,
        terminal_tick: None,
        checkpoint_id: None,
        effect_sequence: 0,
        payload: vec![1],
    }
}

fn phase_writer_fences(store: &dyn PlatformStore, phase: u8) {
    let (claim, execution) = running(store);
    let original = ProviderStateRecord {
        namespace: "native-root-owned-test".into(),
        key: "existing".into(),
        version: 1,
        payload: vec![1],
    };
    store.put_provider_state(original.clone(), None).unwrap();
    match phase {
        0 => {}
        1 => {
            store.close_root_driver(&claim, &execution, 10).unwrap();
        }
        2 => {
            let request = publication(store, &claim, &execution, true);
            store.commit_root_terminal_step(request).unwrap();
        }
        3 => {
            store.fence_root_driver(&claim, &execution, 10).unwrap();
        }
        _ => unreachable!(),
    }
    let epoch = store.provider_state_retention_epoch().unwrap();
    let events = store.events(&execution.execution_id, 1, 32).unwrap();
    let notifications = store.pending_notifications(32).unwrap();
    let core = store.get_execution(&execution.execution_id).unwrap();
    assert!(store.enqueue(work(&execution)).is_err());
    assert!(store.put_checkpoint(checkpoint(&execution)).is_err());
    assert!(
        store
            .append_event(event(
                &execution,
                events.len() as u64 + 1,
                LifecycleEventKind::Completing
            ))
            .is_err()
    );
    assert!(
        store
            .transition_execution(
                &execution.execution_id,
                execution.version,
                ExecutionState::Completing,
                10
            )
            .is_err()
    );
    // The reserved ownership namespace cannot be forged even while Open.
    let forged = ProviderStateRecord {
        namespace: "durable-root-actor-v1".into(),
        key: "forged-member".into(),
        version: 1,
        payload: b"native-root".to_vec(),
    };
    assert!(store.put_provider_state(forged.clone(), None).is_err());
    assert!(
        store
            .mutate_provider_states_atomic(vec![
                ProviderStateMutation::Put(ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: "unrelated-test".into(),
                        key: "must-rollback".into(),
                        version: 1,
                        payload: vec![1]
                    },
                    expected_version: None
                }),
                ProviderStateMutation::Put(ProviderStateWrite {
                    record: forged,
                    expected_version: None
                }),
            ])
            .is_err()
    );
    assert!(
        store
            .get_provider_state("unrelated-test", "must-rollback")
            .unwrap()
            .is_none()
    );
    if phase != 0 {
        let mut next = original.clone();
        next.version = 2;
        assert!(store.put_provider_state(next.clone(), Some(1)).is_err());
        assert!(
            store
                .delete_provider_state(&original.namespace, &original.key, 1)
                .is_err()
        );
        next.key = "moved".into();
        assert!(
            store
                .move_provider_state(next.clone(), &original.key, 1)
                .is_err()
        );
        assert!(
            store
                .mutate_provider_states_atomic(vec![ProviderStateMutation::Move {
                    record: next,
                    old_key: original.key.clone(),
                    expected_version: 1
                }])
                .is_err()
        );
        assert!(
            store
                .append_notification(outbox(&event(
                    &execution,
                    events.len() as u64 + 1,
                    LifecycleEventKind::Started
                )))
                .is_err()
        );
        let mut intent = orphan_intent();
        intent.execution_id = execution.execution_id.clone();
        intent.intent.owner = execution.execution_id.clone();
        assert!(store.record_intent(intent.clone()).is_err());
        assert!(store.effect(&intent.key).unwrap().is_none());
    }
    assert_eq!(
        store
            .get_provider_state(&original.namespace, &original.key)
            .unwrap(),
        Some(original)
    );
    assert_eq!(store.get_execution(&execution.execution_id).unwrap(), core);
    assert_eq!(
        store.events(&execution.execution_id, 1, 32).unwrap(),
        events
    );
    assert_eq!(store.pending_notifications(32).unwrap(), notifications);
    assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
    let notification = &notifications[0];
    if phase == 1 {
        assert_eq!(
            store.mark_notification_delivered(
                &notification.notification_id,
                notification.version,
                10
            ),
            Err(StoreError::InvalidTransition)
        );
        assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
    } else {
        // Exact delivery does not insert a notification or grant execution.
        let delivered = store
            .mark_notification_delivered(&notification.notification_id, notification.version, 10)
            .unwrap();
        assert!(delivered.delivered);
        assert_eq!(delivered.payload, notification.payload);
    }
}

#[test]
fn memory_child_normal_completion() {
    child_completion_remains_valid(&MemoryStore::new(StoreLimits::default()), false);
}
#[test]
fn memory_child_abend() {
    child_completion_remains_valid(&MemoryStore::new(StoreLimits::default()), true);
}
#[test]
fn sqlite_child_normal_completion() {
    let fixture = OwnedSqlite::new();
    child_completion_remains_valid(fixture.store(), false);
}
#[test]
fn sqlite_child_abend() {
    let fixture = OwnedSqlite::new();
    child_completion_remains_valid(fixture.store(), true);
}
adapter_tests!(
    memory_legacy_checkpoint_and_events,
    sqlite_legacy_checkpoint_and_events,
    legacy_checkpoint_and_events_remain_valid
);
#[test]
fn memory_all_writer_phases() {
    for phase in 0..4 {
        phase_writer_fences(&MemoryStore::new(StoreLimits::default()), phase);
    }
}
#[test]
fn sqlite_all_writer_phases() {
    for phase in 0..4 {
        let fixture = OwnedSqlite::new();
        phase_writer_fences(fixture.store(), phase);
    }
}

fn closing_direct_audit_refuses(store: &dyn PlatformStore) {
    let (claim, execution) = running(store);
    let request = publication(store, &claim, &execution, true);
    let before = store.provider_state_retention_epoch().unwrap();
    // A diagnostic authorization-denial fixture, not native terminal proof.
    let audit = AuditRecord {
        execution_id: execution.execution_id.clone(),
        run_unit_id: execution.run_unit_id.clone(),
        principal: execution.principal.clone(),
        attempt: execution.attempt,
        effect_sequence: 1,
        invocation_key: claim.admission().invocation_key.clone(),
        capability: CapabilityId::new("host.mq.write", InvocationLimits::default()).unwrap(),
        resource: AuditResourceDigest {
            format: AuditResourceDigestFormat::CanonicalHostResourceV1,
            value: [2; 32],
        },
        decision: AuditDecision::Deny,
        observed_tick: 10,
    };
    assert_eq!(
        store.record_audit(audit),
        Err(StoreError::InvalidTransition)
    );
    assert_eq!(store.provider_state_retention_epoch().unwrap(), before);
    assert!(
        store
            .audit_subject_records(&execution.execution_id, 32)
            .unwrap()
            .is_empty()
    );
    store.commit_root_terminal_step(request).unwrap();
}
adapter_tests!(
    memory_closing_direct_audit,
    sqlite_closing_direct_audit,
    closing_direct_audit_refuses
);

fn copied_terminal_audit_refuses(store: &dyn PlatformStore) {
    let (claim, execution) = running(store);
    let request = publication(store, &claim, &execution, true);
    store.commit_root_terminal_step(request).unwrap();
    let original = store.list_provider_state("durable-audit-v1", 32).unwrap();
    assert_eq!(original.len(), 2);
    let mut copied = original[0].clone();
    copied.key = "copied-terminal-subject".into();
    let epoch = store.provider_state_retention_epoch().unwrap();
    // Copy a real committed subject; do not fabricate native finality from an
    // application effect or construct the private terminal codec in a test.
    assert!(store.put_provider_state(copied, None).is_err());
    assert_eq!(
        store.list_provider_state("durable-audit-v1", 32).unwrap(),
        original
    );
    assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
}
adapter_tests!(
    memory_copied_terminal_audit,
    sqlite_copied_terminal_audit,
    copied_terminal_audit_refuses
);
