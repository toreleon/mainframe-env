use crate::{MemoryStore, SqliteStateStore, StoreLimits};
use mainframe_env_execution_api::{
    ArtifactRef, AuditDecision, AuditRecord, AuditResourceDigest, AuditResourceDigestFormat,
    CapabilityId, ExecutionId, IdempotencyKey, InvocationLimits, PrincipalId, RunUnitId, Selector,
};
use mainframe_env_store_api::*;
use std::sync::{Arc, Barrier};

#[test]
fn audited_publication_successful_move_and_delete_use_the_existing_row_cas() {
    for store in backends() {
        let mut request = prepare(store.as_ref());
        store
            .mutate_provider_states_atomic(vec![
                put("delete", 1, None, b"old"),
                put("move", 1, None, b"before"),
            ])
            .unwrap();
        request.mutations = vec![
            ProviderStateMutation::Delete {
                namespace: "publication-fixture-v1".into(),
                key: "delete".into(),
                expected_version: 1,
            },
            ProviderStateMutation::Move {
                record: ProviderStateRecord {
                    namespace: "publication-fixture-v1".into(),
                    key: "moved".into(),
                    version: 2,
                    payload: b"after".to_vec(),
                },
                old_key: "move".into(),
                expected_version: 1,
            },
        ];
        store
            .publish_provider_states_audited(request.clone())
            .unwrap();
        assert_eq!(
            store
                .get_provider_state("publication-fixture-v1", "delete")
                .unwrap(),
            None
        );
        assert_eq!(
            store
                .get_provider_state("publication-fixture-v1", "move")
                .unwrap(),
            None
        );
        let row = store
            .get_provider_state("publication-fixture-v1", "moved")
            .unwrap()
            .unwrap();
        assert_eq!((row.version, row.payload), (2, b"after".to_vec()));
        assert_eq!(
            store
                .audit_records(&request.intent.execution_id, 1, 8)
                .unwrap(),
            vec![request.audit]
        );
    }
}

#[test]
fn audited_publication_coherent_foreign_sequence_and_oversized_payload_fail() {
    for store in backends() {
        let request = prepare(store.as_ref());
        let before = baseline(store.as_ref(), &request);
        for mutant in 0..3 {
            let mut changed = request.clone();
            match mutant {
                0 => {
                    changed.intent.sequence = 4;
                    changed.audit.effect_sequence = 4;
                }
                1 => {
                    changed.intent.intent.attempt = 2;
                    changed.audit.attempt = 2;
                }
                2 => changed.mutations = vec![put("queue", 1, None, &vec![1; 8193])],
                _ => unreachable!(),
            }
            let expected = if mutant == 2 {
                StoreError::PayloadTooLarge
            } else {
                StoreError::Conflict
            };
            assert_eq!(
                store.publish_provider_states_audited(changed),
                Err(expected)
            );
            unchanged(store.as_ref(), &request, &before);
        }
    }
}

pub(crate) fn prepare(store: &dyn PlatformStore) -> AuditedProviderPublication {
    let l = InvocationLimits::default();
    let execution = ExecutionRecord {
        execution_id: ExecutionId::new("publication-exec", l).unwrap(),
        run_unit_id: RunUnitId::new("publication-run", l).unwrap(),
        principal: PrincipalId::new("ISSUER", l).unwrap(),
        selector: Selector::new("program:PUBLICATION", l).unwrap(),
        artifact: ArtifactRef::new(format!("sha256:{}", "a".repeat(64)), l).unwrap(),
        state: ExecutionState::Admitted,
        attempt: 1,
        version: 1,
        owner_lease: None,
        lease_expiry_tick: None,
        terminal_tick: None,
    };
    store.create_execution(execution.clone()).unwrap();
    store
        .transition_execution(&execution.execution_id, 1, ExecutionState::Queued, 6)
        .unwrap();
    store
        .transition_execution(&execution.execution_id, 2, ExecutionState::Running, 7)
        .unwrap();
    let audit = AuditRecord {
        execution_id: execution.execution_id.clone(),
        run_unit_id: execution.run_unit_id.clone(),
        attempt: 1,
        effect_sequence: 3,
        observed_tick: 9,
        principal: execution.principal,
        invocation_key: IdempotencyKey::new("invocation:publication", l).unwrap(),
        capability: CapabilityId::new("host.mq", l).unwrap(),
        resource: AuditResourceDigest {
            format: AuditResourceDigestFormat::CanonicalHostResourceV1,
            value: [0x51; 32],
        },
        decision: AuditDecision::Success,
    };
    let intent = EffectRecord {
        execution_id: execution.execution_id.clone(),
        run_unit_id: execution.run_unit_id,
        sequence: 3,
        key: IdempotencyKey::new("effect:publication:3", l).unwrap(),
        digest_format: EffectDigestFormat::CanonicalHostV1,
        request_digest: [0x31; 32],
        intent: EffectIntentMetadata {
            owner: execution.execution_id,
            attempt: 1,
            capability: Some(audit.capability.clone()),
            audit_resource: Some(audit.resource),
            audit_invocation_key: Some(audit.invocation_key.clone()),
            created_tick: 8,
            recovery_after_tick: 30,
            epoch: 4,
            recovery_lease: None,
        },
        state: EffectState::Intent,
        result_digest: None,
        resolved_tick: None,
    };
    store.record_intent(intent.clone()).unwrap();
    AuditedProviderPublication {
        intent,
        audit,
        observed_tick: 9,
        mutations: vec![put("queue", 1, None, b"published")],
    }
}

pub(crate) fn put(
    key: &str,
    version: u64,
    expected: Option<u64>,
    bytes: &[u8],
) -> ProviderStateMutation {
    ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: "publication-fixture-v1".into(),
            key: key.into(),
            version,
            payload: bytes.to_vec(),
        },
        expected_version: expected,
    })
}

fn backends() -> Vec<Arc<dyn PlatformStore>> {
    vec![
        Arc::new(MemoryStore::new(StoreLimits {
            max_blob_bytes: 8192,
            ..StoreLimits::default()
        })),
        Arc::new(SqliteStateStore::open("sqlite::memory:", 8192, 64).unwrap()),
    ]
}

fn baseline(
    store: &dyn PlatformStore,
    request: &AuditedProviderPublication,
) -> (u64, ExecutionRecord) {
    (
        store.provider_state_retention_epoch().unwrap(),
        store
            .get_execution(&request.intent.execution_id)
            .unwrap()
            .unwrap(),
    )
}

fn unchanged(
    store: &dyn PlatformStore,
    request: &AuditedProviderPublication,
    before: &(u64, ExecutionRecord),
) {
    assert_eq!(store.provider_state_retention_epoch().unwrap(), before.0);
    assert_eq!(
        store.get_execution(&request.intent.execution_id).unwrap(),
        Some(before.1.clone())
    );
    assert_eq!(
        store.effect(&request.intent.key).unwrap(),
        Some(request.intent.clone())
    );
    assert_eq!(
        store
            .get_provider_state("publication-fixture-v1", "queue")
            .unwrap(),
        None
    );
    assert!(
        store
            .audit_records(&request.intent.execution_id, 1, 8)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn audited_publication_rows_and_audit_commit_without_coordinator_completion() {
    for store in backends() {
        let mut request = prepare(store.as_ref());
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "publication-fixture-v1".into(),
                    key: "untouched".into(),
                    version: 1,
                    payload: b"original".to_vec(),
                },
                None,
            )
            .unwrap();
        let untouched = store
            .get_provider_state("publication-fixture-v1", "untouched")
            .unwrap();
        let execution = store.get_execution(&request.intent.execution_id).unwrap();
        request.mutations.push(put("uow", 1, None, b"staged-work"));
        request
            .mutations
            .push(put("replay", 1, None, b"known-result"));
        store
            .publish_provider_states_audited(request.clone())
            .unwrap();
        assert_eq!(
            store
                .get_provider_state("publication-fixture-v1", "queue")
                .unwrap()
                .unwrap()
                .payload,
            b"published"
        );
        assert_eq!(
            store
                .get_provider_state("publication-fixture-v1", "replay")
                .unwrap()
                .unwrap()
                .payload,
            b"known-result"
        );
        assert_eq!(
            store
                .audit_records(&request.intent.execution_id, 1, 8)
                .unwrap(),
            vec![request.audit.clone()]
        );
        assert_eq!(
            store
                .get_provider_state("publication-fixture-v1", "untouched")
                .unwrap(),
            untouched
        );
        assert_eq!(
            store.effect(&request.intent.key).unwrap(),
            Some(request.intent.clone())
        );
        assert_eq!(
            store.get_execution(&request.intent.execution_id).unwrap(),
            execution
        );
        assert!(
            store
                .events(&request.intent.execution_id, 1, 8)
                .unwrap()
                .is_empty()
        );
        assert!(store.pending_notifications(8).unwrap().is_empty());
        assert_eq!(
            store.publish_provider_states_audited(request.clone()),
            Err(StoreError::Conflict)
        );
        assert_eq!(
            store
                .audit_records(&request.intent.execution_id, 1, 8)
                .unwrap()
                .len(),
            1
        );
    }
}

#[test]
fn audited_publication_audit_only_deny_and_failure_preserve_sink_ordering() {
    for store in backends() {
        let mut request = prepare(store.as_ref());
        request.mutations.clear();
        request.audit.decision = AuditDecision::Deny;
        store
            .publish_provider_states_audited(request.clone())
            .unwrap();
        let mut failure = request.clone();
        failure.observed_tick = 10;
        failure.audit.observed_tick = 10;
        failure.audit.decision = AuditDecision::ProviderFailure;
        store
            .publish_provider_states_audited(failure.clone())
            .unwrap();
        let mut direct = request.audit.clone();
        direct.decision = AuditDecision::Rejected;
        store.record_audit(direct.clone()).unwrap();
        assert_eq!(
            store
                .audit_records(&request.intent.execution_id, 1, 8)
                .unwrap(),
            vec![request.audit.clone(), failure.audit, direct]
        );
        assert_eq!(
            store
                .get_provider_state("publication-fixture-v1", "queue")
                .unwrap(),
            None
        );
        assert_eq!(
            store.effect(&request.intent.key).unwrap(),
            Some(request.intent)
        );
    }
}

#[test]
fn audited_publication_deny_core_rows_and_oversized_batches_fail_unchanged() {
    for store in backends() {
        let request = prepare(store.as_ref());
        let before = baseline(store.as_ref(), &request);
        let mut deny = request.clone();
        deny.audit.decision = AuditDecision::Deny;
        assert_eq!(
            store.publish_provider_states_audited(deny),
            Err(StoreError::InvalidTransition)
        );
        for namespace in [
            "durable-effect",
            "durable-execution",
            "durable-audit-v1",
            "durable-outbox",
            "durable-event:publication-exec",
        ] {
            let mut forged = request.clone();
            if let ProviderStateMutation::Put(write) = &mut forged.mutations[0] {
                write.record.namespace = namespace.into();
            }
            assert_eq!(
                store.publish_provider_states_audited(forged),
                Err(StoreError::InvalidTransition)
            );
        }
        let mut too_many = request.clone();
        too_many.mutations = vec![put("row", 1, None, b"x"); MAX_AUDITED_PROVIDER_MUTATIONS + 1];
        assert_eq!(
            store.publish_provider_states_audited(too_many),
            Err(StoreError::CapacityExceeded)
        );
        unchanged(store.as_ref(), &request, &before);
    }
}

#[test]
fn audited_publication_late_conflict_rolls_back_put_delete_move_and_audit() {
    for store in backends() {
        let mut request = prepare(store.as_ref());
        for key in ["delete", "move"] {
            store
                .mutate_provider_states_atomic(vec![put(key, 1, None, key.as_bytes())])
                .unwrap();
        }
        let before = baseline(store.as_ref(), &request);
        request.mutations.extend([
            ProviderStateMutation::Delete {
                namespace: "publication-fixture-v1".into(),
                key: "delete".into(),
                expected_version: 1,
            },
            ProviderStateMutation::Move {
                record: ProviderStateRecord {
                    namespace: "publication-fixture-v1".into(),
                    key: "moved".into(),
                    version: 2,
                    payload: b"moved".to_vec(),
                },
                old_key: "move".into(),
                expected_version: 1,
            },
            put("queue", 8, Some(7), b"conflict"),
        ]);
        assert_eq!(
            store.publish_provider_states_audited(request.clone()),
            Err(StoreError::Conflict)
        );
        unchanged(store.as_ref(), &request, &before);
        assert_eq!(
            store
                .get_provider_state("publication-fixture-v1", "delete")
                .unwrap()
                .unwrap()
                .version,
            1
        );
        assert_eq!(
            store
                .get_provider_state("publication-fixture-v1", "move")
                .unwrap()
                .unwrap()
                .payload,
            b"move"
        );
        assert_eq!(
            store
                .get_provider_state("publication-fixture-v1", "moved")
                .unwrap(),
            None
        );
    }
}

#[test]
fn audited_publication_audit_attribution_mutants_fail_closed() {
    for store in backends() {
        let request = prepare(store.as_ref());
        let before = baseline(store.as_ref(), &request);
        for mutant in 0..10 {
            let mut changed = request.clone();
            let l = InvocationLimits::default();
            match mutant {
                0 => changed.audit.execution_id = ExecutionId::new("foreign-exec", l).unwrap(),
                1 => changed.audit.run_unit_id = RunUnitId::new("foreign-run", l).unwrap(),
                2 => changed.audit.attempt = 2,
                3 => changed.audit.effect_sequence = 4,
                4 => changed.audit.capability = CapabilityId::new("host.foreign", l).unwrap(),
                5 => changed.audit.resource.value[0] ^= 1,
                6 => {
                    changed.audit.invocation_key =
                        IdempotencyKey::new("foreign-invocation", l).unwrap()
                }
                7 => changed.audit.principal = PrincipalId::new("OTHER", l).unwrap(),
                8 => {
                    changed.audit.resource.format =
                        AuditResourceDigestFormat::CanonicalHostOversizedResourceV1
                }
                9 => changed.audit.observed_tick = 10,
                _ => unreachable!(),
            }
            assert_eq!(
                store.publish_provider_states_audited(changed),
                Err(StoreError::Conflict),
                "mutant {mutant}"
            );
            unchanged(store.as_ref(), &request, &before);
        }
    }
}

#[test]
fn audited_publication_intent_identity_and_missing_legacy_terminal_mutants_fail() {
    for store in backends() {
        let request = prepare(store.as_ref());
        let before = baseline(store.as_ref(), &request);
        for mutant in 0..13 {
            let mut changed = request.clone();
            let l = InvocationLimits::default();
            match mutant {
                0 => changed.intent.key = IdempotencyKey::new("missing-key", l).unwrap(),
                1 => changed.intent.request_digest[0] ^= 1,
                2 => changed.intent.intent.epoch += 1,
                3 => changed.intent.intent.created_tick += 1,
                4 => changed.intent.intent.recovery_after_tick += 1,
                5 => changed.intent.intent.owner = ExecutionId::new("foreign-owner", l).unwrap(),
                6 => changed.intent.intent.attempt += 1,
                7 => changed.intent.digest_format = EffectDigestFormat::LegacyDebug,
                8 => {
                    changed.intent.state = EffectState::Completed;
                    changed.intent.result_digest = Some([0x41; 32]);
                    changed.intent.resolved_tick = Some(9);
                }
                9 => {
                    changed.intent.state = EffectState::UnknownOutcome;
                    changed.intent.result_digest = Some([0x41; 32]);
                }
                10 => changed.intent.intent.audit_resource = None,
                11 => changed.intent.intent.audit_invocation_key = None,
                12 => changed.intent.intent.capability = None,
                _ => unreachable!(),
            }
            assert!(
                store.publish_provider_states_audited(changed).is_err(),
                "mutant {mutant}"
            );
            unchanged(store.as_ref(), &request, &before);
        }
    }
}

#[test]
fn audited_publication_clock_and_expiry_fail_without_audit_or_row_changes() {
    for store in backends() {
        let request = prepare(store.as_ref());
        let before = baseline(store.as_ref(), &request);
        for tick in [0, 7, 30, u64::MAX] {
            let mut changed = request.clone();
            changed.observed_tick = tick;
            changed.audit.observed_tick = tick;
            assert_eq!(
                store.publish_provider_states_audited(changed),
                Err(StoreError::LeaseConflict)
            );
            unchanged(store.as_ref(), &request, &before);
        }
        store.advance_logical_clock(10).unwrap();
        assert_eq!(
            store.publish_provider_states_audited(request.clone()),
            Err(StoreError::LeaseConflict)
        );
        unchanged(store.as_ref(), &request, &before);
    }
}

#[test]
fn audited_publication_recovery_claim_and_resolution_fence_original_dispatcher() {
    for store in backends() {
        let request = prepare(store.as_ref());
        let claimed = store
            .claim_stale_intent(&request.intent.key, 4, "resolver", 30, 1, 10)
            .unwrap();
        let epoch = store.provider_state_retention_epoch().unwrap();
        assert_eq!(
            store.publish_provider_states_audited(request.clone()),
            Err(StoreError::Conflict)
        );
        assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
        assert_eq!(
            store
                .get_provider_state("publication-fixture-v1", "queue")
                .unwrap(),
            None
        );
        assert!(
            store
                .audit_records(&request.intent.execution_id, 1, 8)
                .unwrap()
                .is_empty()
        );
        let mut recovery = request.clone();
        recovery.intent = claimed.clone();
        assert_eq!(
            store.publish_provider_states_audited(recovery),
            Err(StoreError::LeaseConflict)
        );
        store
            .reconcile_stale_intent(
                &request.intent.key,
                "resolver",
                claimed.intent.recovery_lease.unwrap().epoch,
                31,
                EffectState::Completed,
                EffectDigestFormat::CanonicalHostV1,
                [0x71; 32],
            )
            .unwrap();
        let epoch = store.provider_state_retention_epoch().unwrap();
        let audits = store
            .audit_records(&request.intent.execution_id, 1, 8)
            .unwrap();
        assert_eq!(
            store.publish_provider_states_audited(request.clone()),
            Err(StoreError::Conflict)
        );
        assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
        assert_eq!(
            store
                .audit_records(&request.intent.execution_id, 1, 8)
                .unwrap(),
            audits
        );
    }
}

#[test]
fn audited_publication_coordinator_resolution_and_terminal_execution_reject() {
    for store in backends() {
        let request = prepare(store.as_ref());
        let result = EffectRecord {
            state: EffectState::Completed,
            result_digest: Some([0x71; 32]),
            resolved_tick: Some(9),
            ..request.intent.clone()
        };
        store
            .record_result(&request.intent.key, result.clone())
            .unwrap();
        assert_eq!(
            store.publish_provider_states_audited(request.clone()),
            Err(StoreError::Conflict)
        );
        assert_eq!(store.effect(&request.intent.key).unwrap(), Some(result));
        assert!(
            store
                .audit_records(&request.intent.execution_id, 1, 8)
                .unwrap()
                .is_empty()
        );
    }
    for store in backends() {
        let request = prepare(store.as_ref());
        store
            .transition_execution(&request.intent.execution_id, 3, ExecutionState::Failed, 9)
            .unwrap();
        assert_eq!(
            store.publish_provider_states_audited(request.clone()),
            Err(StoreError::Conflict)
        );
        assert_eq!(
            store
                .get_provider_state("publication-fixture-v1", "queue")
                .unwrap(),
            None
        );
        assert!(
            store
                .audit_records(&request.intent.execution_id, 1, 8)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn audited_publication_same_fence_concurrent_row_cas_has_one_audited_winner() {
    for store in backends() {
        let mut request = prepare(store.as_ref());
        store
            .mutate_provider_states_atomic(vec![put("queue", 1, None, b"before")])
            .unwrap();
        request.mutations = vec![put("queue", 2, Some(1), b"after")];
        let barrier = Arc::new(Barrier::new(3));
        let threads: Vec<_> = (0..2)
            .map(|_| {
                let store = store.clone();
                let request = request.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.publish_provider_states_audited(request)
                })
            })
            .collect();
        barrier.wait();
        let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|r| **r == Err(StoreError::Conflict))
                .count(),
            1
        );
        assert_eq!(
            store
                .get_provider_state("publication-fixture-v1", "queue")
                .unwrap()
                .unwrap()
                .version,
            2
        );
        assert_eq!(
            store
                .audit_records(&request.intent.execution_id, 1, 8)
                .unwrap(),
            vec![request.audit]
        );
        assert_eq!(
            store.effect(&request.intent.key).unwrap(),
            Some(request.intent)
        );
    }
}
