//! Physical corruption fixtures only; no executable or root admission authority.
use super::*;
use mainframe_env_execution_api::{
    AuditDecision, AuditResourceDigest, AuditResourceDigestFormat, CapabilityId,
    LifecycleEventKind, PrincipalId, RunUnitId, Selector,
};

fn footprint(store: &MemoryStore) -> String {
    let state = store.lock().unwrap();
    format!(
        "{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}",
        state.executions,
        state.events,
        state.work,
        state.checkpoints,
        state.sessions,
        state.artifacts,
        state.generations,
        state.effects,
        state.audits,
        state.next_audit_ordinal,
        state.outbox,
        state.provider_state,
        state.blob_bytes,
        state.archives,
        state.archive_bytes,
        state.observations,
        state.observation_bytes,
        state.retention_cas_versions,
        state.provider_epoch,
        state.logical_tick,
        store.limits,
        store.clone_count.load(std::sync::atomic::Ordering::Relaxed)
    )
}

fn physical_row(store: &MemoryStore, namespace: &str, key: &str, declared: &str, payload: &[u8]) {
    // Deliberately mismatched identity, zero version and arbitrary bytes. Only the
    // map tuple identifies these rows for the existing count/existence predicates.
    store.lock().unwrap().provider_state.insert(
        (namespace.into(), key.into()),
        ProviderStateRecord {
            namespace: declared.into(),
            key: "mismatched-value-key".into(),
            version: 0,
            payload: payload.into(),
        },
    );
}

fn audit(sequence: u64) -> AuditRecord {
    let bounds = InvocationLimits::default();
    AuditRecord {
        execution_id: ExecutionId::new("namespace-query-execution", bounds).unwrap(),
        run_unit_id: RunUnitId::new("namespace-query-run", bounds).unwrap(),
        attempt: 1,
        effect_sequence: sequence,
        observed_tick: 2,
        principal: PrincipalId::new("IBMUSER", bounds).unwrap(),
        invocation_key: IdempotencyKey::new("namespace-query-invocation", bounds).unwrap(),
        capability: CapabilityId::new("host.state.write", bounds).unwrap(),
        resource: AuditResourceDigest {
            format: AuditResourceDigestFormat::CanonicalHostResourceV1,
            value: [9; 32],
        },
        decision: AuditDecision::Success,
    }
}

#[test]
fn memory_audit_quota_keeps_corrupt_physical_rows_and_ignores_adjacent_namespaces() {
    let store = MemoryStore::new(StoreLimits {
        max_audits: 3,
        ..StoreLimits::default()
    });
    for key in ["", "é"] {
        physical_row(
            &store,
            AUDIT_NAMESPACE,
            key,
            "other-value-namespace",
            b"not-json",
        );
    }
    for namespace in [
        "a".to_string(),
        format!("{AUDIT_NAMESPACE}\0"),
        format!("{AUDIT_NAMESPACE}-suffix"),
        "é".to_string(),
    ] {
        physical_row(&store, &namespace, "foreign", AUDIT_NAMESPACE, b"not-json");
    }
    // Invalid values at the exact physical namespace still consume two slots.
    // Adjacent namespaces declaring audit values consume none of those slots.
    store.record_audit(audit(1)).unwrap();
    let before = footprint(&store);
    assert_eq!(
        store.record_audit(audit(2)),
        Err(StoreError::CapacityExceeded)
    );
    let mut invalid = audit(2);
    invalid.attempt = 0;
    // The original validation-before-quota refusal order is unchanged.
    assert_eq!(
        store.record_audit(invalid),
        Err(StoreError::InvalidTransition)
    );
    assert_eq!(footprint(&store), before);
}

#[test]
fn memory_run_presence_fences_use_physical_identity_with_empty_key_and_corrupt_values() {
    let store = MemoryStore::new(StoreLimits::default());
    for namespace in [
        "durable-root-run-v0".to_string(),
        format!("{RUN_NAMESPACE}\0"),
        format!("{RUN_NAMESPACE}-suffix"),
        "é".to_string(),
    ] {
        physical_row(&store, &namespace, "run", RUN_NAMESPACE, b"foreign-root");
    }
    let before = footprint(&store);
    assert_eq!(
        guard_provider(
            &store.lock().unwrap(),
            AUDIT_NAMESPACE,
            "audit",
            Some(b"not-json")
        ),
        Ok(())
    );
    assert_eq!(footprint(&store), before);
    // Even this malformed, empty-key physical row enables the original audit fence.
    physical_row(
        &store,
        RUN_NAMESPACE,
        "",
        "foreign-value-namespace",
        b"\xff",
    );
    let before = footprint(&store);
    assert_eq!(
        guard_provider(
            &store.lock().unwrap(),
            AUDIT_NAMESPACE,
            "audit",
            Some(b"not-json")
        ),
        Err(StoreError::IncompatibleVersion)
    );
    assert_eq!(
        store.put_provider_state(
            ProviderStateRecord {
                namespace: AUDIT_NAMESPACE.into(),
                key: "attempted-audit".into(),
                version: 1,
                payload: b"not-json".to_vec(),
            },
            None,
        ),
        Err(StoreError::IncompatibleVersion)
    );
    assert_eq!(footprint(&store), before);

    physical_row(
        &store,
        RUN_NAMESPACE,
        "run",
        "foreign-value-namespace",
        b"\xff",
    );
    let before = footprint(&store);
    for namespace in [
        "cobol-call-replay@1",
        "cobol-call-protocol@2",
        "cobol-run-state@1",
        "cobol-cancel@1",
    ] {
        assert_eq!(
            store.put_provider_state(
                ProviderStateRecord {
                    namespace: namespace.into(),
                    key: "attempted-call".into(),
                    version: 1,
                    payload: br#"{"owner_run_unit":"run"}"#.to_vec(),
                },
                None,
            ),
            Err(StoreError::IncompatibleVersion)
        );
        // Malformed unowned legacy documents still keep their original decoder authority.
        assert_eq!(
            guard_provider(&store.lock().unwrap(), namespace, "call", Some(b"not-json")),
            Ok(())
        );
    }
    assert_eq!(
        guard_provider(
            &store.lock().unwrap(),
            "other",
            "call",
            Some(br#"{"owner_run_unit":"run"}"#)
        ),
        Ok(())
    );
    assert_eq!(
        guard_provider(
            &store.lock().unwrap(),
            RUN_NAMESPACE,
            "call",
            Some(b"not-json")
        ),
        Err(StoreError::InvalidTransition)
    );
    assert_eq!(footprint(&store), before);
}

#[test]
fn memory_audit_count_keeps_late_journal_failure_whole_state_and_quota_unchanged() {
    let store = MemoryStore::new(StoreLimits {
        max_audits: 2,
        max_outbox: 1,
        ..StoreLimits::default()
    });
    physical_row(
        &store,
        AUDIT_NAMESPACE,
        "",
        "foreign-value-namespace",
        b"not-json",
    );
    physical_row(
        &store,
        &format!("{AUDIT_NAMESPACE}\0"),
        "foreign",
        AUDIT_NAMESPACE,
        b"not-json",
    );
    let record = audit(1);
    let execution = ExecutionRecord {
        execution_id: record.execution_id.clone(),
        run_unit_id: record.run_unit_id.clone(),
        principal: record.principal.clone(),
        selector: Selector::new("program:QUERY", InvocationLimits::default()).unwrap(),
        artifact: ArtifactRef::new("sha256:namespace-query", InvocationLimits::default()).unwrap(),
        state: ExecutionState::Admitted,
        attempt: 1,
        version: 1,
        owner_lease: None,
        lease_expiry_tick: None,
        terminal_tick: None,
    };
    let event = |sequence, kind| LifecycleEvent {
        execution_id: execution.execution_id.clone(),
        run_unit_id: execution.run_unit_id.clone(),
        sequence,
        attempt: 1,
        tick: 2,
        kind,
    };
    let notification = |sequence| OutboxRecord {
        notification_id: format!("namespace-query-{sequence}"),
        execution_id: execution.execution_id.clone(),
        sequence,
        topic: "execution.lifecycle".into(),
        payload: vec![1],
        attempt: 0,
        delivered: false,
        delivered_tick: None,
        version: 1,
    };
    store
        .admit_execution(
            execution.clone(),
            event(1, LifecycleEventKind::Admitted),
            notification(1),
        )
        .unwrap();
    let before = footprint(&store);
    assert_eq!(
        store.commit_execution_step(
            &execution.execution_id,
            1,
            None,
            event(2, LifecycleEventKind::EffectResult { sequence: 1 }),
            None,
            Some(record.clone()),
            None,
            notification(2)
        ),
        Err(StoreError::CapacityExceeded)
    );
    // The outbox refusal follows the staged audit/event and must undo every field.
    assert_eq!(footprint(&store), before);
    // The failed audit consumed no quota slot or ordinal; the same slot remains usable.
    store.record_audit(record).unwrap();
    let before = footprint(&store);
    assert_eq!(
        store.record_audit(audit(2)),
        Err(StoreError::CapacityExceeded)
    );
    assert_eq!(footprint(&store), before);
}
