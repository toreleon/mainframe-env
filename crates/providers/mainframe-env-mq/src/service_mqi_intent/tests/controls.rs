use super::*;

#[test]
fn every_audit_identity_is_exact_and_non_success_cannot_mutate() {
    for case in 0..11 {
        for store in backends() {
            let f = Fixture::new();
            f.seed(&*store);
            let scope = f.scope();
            let admission = admit_mqi(&scope, &f.invocation, 10).unwrap();
            let mut bound = bind_core_intent(&admission, &*store, 20).unwrap();
            let mut audit = f.audit(20);
            match case {
                0 => {
                    audit.execution_id =
                        ExecutionId::new("foreign", InvocationLimits::default()).unwrap()
                }
                1 => {
                    audit.run_unit_id =
                        RunUnitId::new("foreign", InvocationLimits::default()).unwrap()
                }
                2 => audit.attempt += 1,
                3 => audit.effect_sequence += 1,
                4 => {
                    audit.principal =
                        PrincipalId::new("FOREIGN", InvocationLimits::default()).unwrap()
                }
                5 => {
                    audit.invocation_key =
                        IdempotencyKey::new("foreign", InvocationLimits::default()).unwrap()
                }
                6 => {
                    audit.capability =
                        CapabilityId::new("host.mq.read", InvocationLimits::default()).unwrap()
                }
                7 => audit.resource.value[0] ^= 1,
                8 => {
                    audit.resource.format =
                        AuditResourceDigestFormat::CanonicalHostOversizedResourceV1
                }
                9 => audit.observed_tick += 1,
                10 => audit.decision = AuditDecision::Deny,
                _ => unreachable!(),
            }
            let epoch = store.provider_state_retention_epoch().unwrap();
            assert!(
                bound
                    .prepare(audit, vec![queue("QUEUE", 1, None)], 20)
                    .is_err(),
                "audit {case}"
            );
            unchanged(&*store, &f, epoch);
            assert_eq!(store.effect(&f.intent().key).unwrap(), Some(f.intent()));
        }
    }
    for store in backends() {
        let f = Fixture::new();
        f.seed(&*store);
        let scope = f.scope();
        let admission = admit_mqi(&scope, &f.invocation, 10).unwrap();
        let mut bound = bind_core_intent(&admission, &*store, 20).unwrap();
        for decision in [
            AuditDecision::Cancelled,
            AuditDecision::TimedOut,
            AuditDecision::Rejected,
            AuditDecision::ProviderFailure,
            AuditDecision::InfrastructureFailure,
        ] {
            let mut audit = f.audit(20);
            audit.decision = decision;
            assert!(
                bound
                    .prepare(audit, vec![queue("QUEUE", 1, None)], 20)
                    .is_err()
            );
        }
        let mut unknown = f.audit(20);
        unknown.decision = AuditDecision::UnknownOutcome;
        assert_eq!(
            bound.prepare(unknown, vec![], 20).err(),
            Some(MqIntentProblem::Host(HostProblem::UnknownOutcome))
        );
        let mut deny = f.audit(20);
        deny.decision = AuditDecision::Deny;
        bound
            .prepare(deny.clone(), vec![], 20)
            .unwrap()
            .publish(20)
            .unwrap();
        assert!(
            store
                .list_provider_state("mq-v1-queue", 8)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            store
                .audit_records(&f.invocation.execution_id, 1, 8)
                .unwrap(),
            vec![deny]
        );
        assert_eq!(store.effect(&f.intent().key).unwrap(), Some(f.intent()));
    }
}

#[test]
fn cancellation_deadlines_and_monotonic_observation_are_live_at_each_boundary() {
    for case in 0..6 {
        for store in backends() {
            let f = Fixture::new();
            f.seed(&*store);
            let scope = f.scope();
            let admission = admit_mqi(&scope, &f.invocation, 10).unwrap();
            let mut bound = bind_core_intent(&admission, &*store, 20).unwrap();
            let epoch = store.provider_state_retention_epoch().unwrap();
            if case == 0 {
                f.invocation.cancellation_probe.as_ref().unwrap().request();
                assert_eq!(
                    bound.prepare(f.audit(20), vec![], 20).err(),
                    Some(MqIntentProblem::Host(HostProblem::Cancelled))
                );
            } else if case == 1 {
                assert_eq!(
                    bound.prepare(f.audit(90), vec![], 90).err(),
                    Some(MqIntentProblem::Host(HostProblem::TimedOut))
                );
                assert_eq!(
                    bound.prepare(f.audit(20), vec![], 20).err(),
                    Some(MqIntentProblem::Host(HostProblem::Malformed))
                );
            } else if case == 2 {
                assert_eq!(
                    bound.prepare(f.audit(19), vec![], 19).err(),
                    Some(MqIntentProblem::Host(HostProblem::Malformed))
                );
            } else {
                let prepared = bound
                    .prepare(f.audit(21), vec![queue("QUEUE", 1, None)], 21)
                    .unwrap();
                let tick = match case {
                    3 => {
                        f.invocation.cancellation_probe.as_ref().unwrap().request();
                        21
                    }
                    4 => 90,
                    5 => 20,
                    _ => unreachable!(),
                };
                assert!(prepared.publish(tick).is_err());
            }
            unchanged(&*store, &f, epoch);
        }
    }
    for store in backends() {
        let f = Fixture::new();
        f.seed(&*store);
        let scope = f.scope();
        let admission = admit_mqi(&scope, &f.invocation, 10).unwrap();
        let mut bound = bind_core_intent(&admission, &*store, 20).unwrap();
        let prepared = bound.prepare(f.audit(21), vec![], 21).unwrap();
        assert!(prepared.publish(22).is_err()); // no silently retimestamped audit
        assert!(bound.prepare(f.audit(21), vec![], 21).is_err()); // observed 22 cannot rewind
        assert_eq!(
            bind_core_intent(&admission, &*store, 90).err().unwrap(),
            MqIntentProblem::Host(HostProblem::TimedOut)
        );
    }
}

#[test]
fn physical_publication_rechecks_recovered_resolved_and_terminal_core_authority() {
    for case in 0..3 {
        for store in backends() {
            let f = Fixture::new();
            f.seed(&*store);
            let scope = f.scope();
            let admission = admit_mqi(&scope, &f.invocation, 10).unwrap();
            let mut bound = bind_core_intent(&admission, &*store, 20).unwrap();
            let prepared = bound
                .prepare(f.audit(21), vec![queue("QUEUE", 1, None)], 21)
                .unwrap();
            match case {
                0 => {
                    store
                        .claim_stale_intent(&f.intent().key, 8, "recovery", 90, 1, 10)
                        .unwrap();
                }
                1 => {
                    let mut resolved = f.intent();
                    resolved.state = EffectState::Completed;
                    resolved.result_digest = Some([1; 32]);
                    resolved.resolved_tick = Some(21);
                    store
                        .record_result(&resolved.key, resolved.clone())
                        .unwrap();
                }
                2 => {
                    store
                        .transition_execution(
                            &f.invocation.execution_id,
                            3,
                            ExecutionState::Failed,
                            21,
                        )
                        .unwrap();
                }
                _ => unreachable!(),
            }
            let retained = store.effect(&f.intent().key).unwrap();
            let execution = store.get_execution(&f.invocation.execution_id).unwrap();
            let epoch = store.provider_state_retention_epoch().unwrap();
            assert!(prepared.publish(21).is_err());
            unchanged(&*store, &f, epoch);
            assert_eq!(store.effect(&f.intent().key).unwrap(), retained);
            assert_eq!(
                store.get_execution(&f.invocation.execution_id).unwrap(),
                execution
            );
            assert!(bind_core_intent(&admission, &*store, 21).is_err());
        }
    }
}

#[test]
fn late_cas_and_backend_payload_quota_roll_back_the_entire_batch_and_audit() {
    for case in 0..2 {
        for store in backends() {
            let f = Fixture::new();
            f.seed(&*store);
            let scope = f.scope();
            let admission = admit_mqi(&scope, &f.invocation, 10).unwrap();
            let mut bound = bind_core_intent(&admission, &*store, 20).unwrap();
            let mut late = queue("MISSING", 2, Some(1));
            if case == 1
                && let ProviderStateMutation::Put(write) = &mut late
            {
                write.record.version = 1;
                write.expected_version = None;
                write.record.payload = vec![0; 8193];
            }
            let epoch = store.provider_state_retention_epoch().unwrap();
            assert!(
                bound
                    .prepare(f.audit(20), vec![queue("FIRST", 1, None), late], 20)
                    .unwrap()
                    .publish(20)
                    .is_err()
            );
            unchanged(&*store, &f, epoch);
            assert_eq!(store.effect(&f.intent().key).unwrap(), Some(f.intent()));
            bound
                .prepare(f.audit(21), vec![queue("FIRST", 1, None)], 21)
                .unwrap()
                .publish(21)
                .unwrap();
            assert_eq!(
                store
                    .audit_records(&f.invocation.execution_id, 1, 8)
                    .unwrap(),
                vec![f.audit(21)]
            );
        }
    }
}

#[test]
fn mq_namespace_batch_and_shared_byte_shapes_fail_closed() {
    for store in backends() {
        let f = Fixture::new();
        f.seed(&*store);
        let scope = f.scope();
        let admission = admit_mqi(&scope, &f.invocation, 10).unwrap();
        let mut bound = bind_core_intent(&admission, &*store, 20).unwrap();
        let epoch = store.provider_state_retention_epoch().unwrap();
        for namespace in [
            "",
            "mq-",
            "mq",
            "ims-v1",
            "durable-effect",
            "jes-worker-meta",
        ] {
            let mut mutation = queue("QUEUE", 1, None);
            if let ProviderStateMutation::Put(write) = &mut mutation {
                write.record.namespace = namespace.into();
            }
            assert!(bound.prepare(f.audit(20), vec![mutation], 20).is_err());
        }
        for mutation in [
            ProviderStateMutation::Delete {
                namespace: "mq-v1-queue".into(),
                key: "".into(),
                expected_version: 1,
            },
            ProviderStateMutation::Delete {
                namespace: "mq-v1-queue".into(),
                key: "QUEUE".into(),
                expected_version: 0,
            },
            ProviderStateMutation::Move {
                record: ProviderStateRecord {
                    namespace: "mq-v1-queue".into(),
                    key: "NEW".into(),
                    version: 2,
                    payload: vec![],
                },
                old_key: "".into(),
                expected_version: 1,
            },
        ] {
            assert!(bound.prepare(f.audit(20), vec![mutation], 20).is_err());
        }
        assert!(
            bound
                .prepare(
                    f.audit(20),
                    vec![queue("QUEUE", 1, None); MAX_AUDITED_PROVIDER_MUTATIONS + 1],
                    20
                )
                .is_err()
        );
        unchanged(&*store, &f, epoch);
    }
    let mut mutation = queue("QUEUE", 1, None);
    if let ProviderStateMutation::Put(write) = &mut mutation {
        write.record.payload = vec![0; MAX_CANONICAL_EFFECT_BYTES];
    }
    assert_eq!(
        bounded_mq_mutations(&[mutation]),
        Err(StoreError::CapacityExceeded)
    );
}

#[test]
fn only_actual_service_validation_is_bindable_and_nested_outer_key_is_not_substituted() {
    for store in backends() {
        let mut f = Fixture::new();
        if let HostRequest::MqMqi(request) = &mut f.effect.request {
            if let MqMqiRequest::Connect(connect) = &mut request.envelope.request {
                connect.options = MqMqiOptions::PendingStructure {
                    requested_version: Some(1),
                };
            }
            let MqMqiRequest::Connect(connect) = &request.envelope.request else {
                unreachable!()
            };
            request.envelope.request = MqMqiRequest::ConnectExtended(connect.clone());
        }
        let scope = f.scope();
        let pending = admit_mqi(&scope, &f.invocation, 10).unwrap();
        assert!(matches!(pending, MqMqiAdmission::Pending { .. }));
        assert_eq!(
            bind_core_intent(&pending, &*store, 20).err(),
            Some(MqIntentProblem::NotServiceValidation)
        );
        let mut f = Fixture::new();
        f.owner.environment = MqHostEnvironment::ZosCics;
        f.invocation.bindings.clear();
        f.invocation.bindings.insert(
            "cics.execution-context".into(),
            BoundedPayload::new(
                "mainframe-env.cics.execution-context@1",
                b"local".to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
        let connection = MqHconn::Default;
        if let HostRequest::MqMqi(request) = &mut f.effect.request {
            request.envelope.context.owner = f.owner;
            request.envelope.context.syncpoint_owner = MqSyncpointOwner::HostCoordinator;
            request.envelope.request = MqMqiRequest::Back {
                connection,
                unit: 1,
            };
        }
        let scope = f.scope();
        let forbidden = admit_mqi(&scope, &f.invocation, 10).unwrap();
        assert!(matches!(forbidden, MqMqiAdmission::ForbiddenContext(_)));
        assert_eq!(
            bind_core_intent(&forbidden, &*store, 20).err(),
            Some(MqIntentProblem::NotServiceValidation)
        );

        let mut f = Fixture::new();
        let key = IdempotencyKey::new("cics:run:7", InvocationLimits::default()).unwrap();
        f.owner.environment = MqHostEnvironment::ZosCics;
        f.invocation.bindings.clear();
        f.invocation.bindings.insert(
            "cics.execution-context".into(),
            BoundedPayload::new(
                "mainframe-env.cics.execution-context@1",
                b"local".to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
        f.effect.idempotency_key = Some(key.clone());
        if let HostRequest::MqMqi(request) = &mut f.effect.request {
            request.mutation.idempotency_key = key.clone();
            request.envelope.context.owner = f.owner;
            request.envelope.context.syncpoint_owner = MqSyncpointOwner::HostCoordinator;
        }
        for (name, schema, bytes) in [
            (
                crate::retention::CICS_NESTED_EFFECT_ORIGIN_BINDING,
                crate::retention::CICS_NESTED_EFFECT_ORIGIN_SCHEMA,
                key.as_str().as_bytes(),
            ),
            (
                crate::retention::CICS_OUTER_EFFECT_ORIGIN_BINDING,
                crate::retention::CICS_OUTER_EFFECT_ORIGIN_SCHEMA,
                b"outer-effect".as_slice(),
            ),
        ] {
            f.invocation.bindings.insert(
                name.into(),
                BoundedPayload::new(schema, bytes.to_vec(), InvocationLimits::default()).unwrap(),
            );
        }
        let scope = f.scope();
        let nested = admit_mqi(&scope, &f.invocation, 10).unwrap();
        assert!(matches!(nested, MqMqiAdmission::ServiceValidation(_)));
        assert_eq!(
            bind_core_intent(&nested, &*store, 20).err(),
            Some(MqIntentProblem::NestedCompositionPending)
        );
        assert!(
            store
                .list_provider_state("mq-v1-queue", 8)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn sqlite_physical_reopen_preserves_published_rows_audit_and_uncompleted_intent() {
    let path = std::env::temp_dir().join(format!("mq-core-intent-{}.sqlite", std::process::id()));
    assert!(!path.exists());
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let f = Fixture::new();
    {
        let store = SqliteStateStore::open(&url, 8192, 64).unwrap();
        f.seed(&store);
        let scope = f.scope();
        let admission = admit_mqi(&scope, &f.invocation, 10).unwrap();
        bind_core_intent(&admission, &store, 20)
            .unwrap()
            .prepare(f.audit(20), vec![queue("QUEUE", 1, None)], 20)
            .unwrap()
            .publish(20)
            .unwrap();
    }
    {
        let store = SqliteStateStore::open(&url, 8192, 64).unwrap();
        assert_eq!(store.effect(&f.intent().key).unwrap(), Some(f.intent()));
        assert_eq!(
            store
                .audit_records(&f.invocation.execution_id, 1, 8)
                .unwrap(),
            vec![f.audit(20)]
        );
        assert_eq!(
            store
                .get_provider_state("mq-v1-queue", "QUEUE")
                .unwrap()
                .unwrap()
                .version,
            1
        );
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn absent_terminal_unknown_and_expired_execution_authority_are_not_filled_in() {
    for case in 0..6 {
        for store in backends() {
            let f = Fixture::new();
            let mut execution = f.execution();
            if case == 5 {
                execution.owner_lease = Some("worker".into());
                execution.lease_expiry_tick = Some(20);
            }
            f.seed_execution(&*store, execution);
            if case != 0 {
                store.record_intent(f.intent()).unwrap();
            }
            if (1..=3).contains(&case) {
                let mut result = f.intent();
                result.state = match case {
                    1 => EffectState::Completed,
                    2 => EffectState::Failed,
                    _ => EffectState::UnknownOutcome,
                };
                result.result_digest = Some([1; 32]);
                result.resolved_tick = (case != 3).then_some(19);
                store.record_result(&result.key, result.clone()).unwrap();
            }
            if case == 4 {
                store
                    .transition_execution(&f.invocation.execution_id, 3, ExecutionState::Failed, 19)
                    .unwrap();
            }
            let before = store.effect(&f.intent().key).unwrap();
            let epoch = store.provider_state_retention_epoch().unwrap();
            let scope = f.scope();
            let admission = admit_mqi(&scope, &f.invocation, 10).unwrap();
            assert!(
                bind_core_intent(&admission, &*store, 20).is_err(),
                "case {case}"
            );
            unchanged(&*store, &f, epoch);
            assert_eq!(store.effect(&f.intent().key).unwrap(), before);
        }
    }
}

#[test]
fn shorter_invocation_deadline_and_one_physical_store_are_preserved() {
    let stores = backends();
    let mut f = Fixture::new();
    f.invocation.deadline_tick = 80;
    for store in &stores {
        f.seed(&**store);
    }
    let scope = f.scope();
    let admission = admit_mqi(&scope, &f.invocation, 10).unwrap();
    let mut bound = bind_core_intent(&admission, &*stores[0], 20).unwrap();
    bound
        .prepare(f.audit(20), vec![queue("QUEUE", 1, None)], 20)
        .unwrap()
        .publish(20)
        .unwrap();
    assert!(
        stores[0]
            .get_provider_state("mq-v1-queue", "QUEUE")
            .unwrap()
            .is_some()
    );
    assert!(
        stores[1]
            .get_provider_state("mq-v1-queue", "QUEUE")
            .unwrap()
            .is_none()
    );
    assert!(
        stores[1]
            .audit_records(&f.invocation.execution_id, 1, 8)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        bound.prepare(f.audit(80), vec![], 80).err(),
        Some(MqIntentProblem::Host(HostProblem::TimedOut))
    );
    assert_eq!(stores[0].effect(&f.intent().key).unwrap(), Some(f.intent()));
}

#[test]
fn late_audit_capacity_failure_restores_rows_epochs_clock_and_core_records() {
    let stores: Vec<Box<dyn PlatformStore>> = vec![
        Box::new(MemoryStore::new(StoreLimits {
            max_audits: 1,
            ..StoreLimits::default()
        })),
        // Two core rows plus an existing audit leave room for a queue,
        // but not the new audit. Memory's single audit slot is likewise full.
        Box::new(SqliteStateStore::open("sqlite::memory:", 8192, 4).unwrap()),
    ];
    for store in stores {
        let f = Fixture::new();
        f.seed(&*store);
        let existing_audit = f.audit(19);
        store.record_audit(existing_audit.clone()).unwrap();
        let before_clock = store.advance_logical_clock(20).unwrap();
        let before_execution = store.get_execution(&f.invocation.execution_id).unwrap();
        let epoch = store.provider_state_retention_epoch().unwrap();
        let scope = f.scope();
        let admission = admit_mqi(&scope, &f.invocation, 10).unwrap();
        let mut bound = bind_core_intent(&admission, &*store, 20).unwrap();
        assert!(
            bound
                .prepare(f.audit(21), vec![queue("QUEUE", 1, None)], 21)
                .unwrap()
                .publish(21)
                .is_err()
        );
        assert!(
            store
                .list_provider_state("mq-v1-queue", 1)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            store
                .audit_records(&f.invocation.execution_id, 1, 1)
                .unwrap(),
            vec![existing_audit]
        );
        assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
        assert_eq!(
            store.advance_logical_clock(before_clock).unwrap(),
            before_clock
        );
        assert_eq!(
            store.get_execution(&f.invocation.execution_id).unwrap(),
            before_execution
        );
        assert_eq!(store.effect(&f.intent().key).unwrap(), Some(f.intent()));
    }
}

#[test]
fn explicit_unknown_never_becomes_a_known_rejection_or_mutation_permission() {
    for store in backends() {
        let f = Fixture::new();
        f.seed(&*store);
        let scope = f.scope();
        let admission = admit_mqi(&scope, &f.invocation, 10).unwrap();
        let mut bound = bind_core_intent(&admission, &*store, 20).unwrap();
        let epoch = store.provider_state_retention_epoch().unwrap();
        f.invocation.cancellation_probe.as_ref().unwrap().request();
        let mut audit = f.audit(0);
        audit.decision = AuditDecision::UnknownOutcome;
        audit.effect_sequence += 1;
        assert_eq!(
            bound
                .prepare(audit, vec![queue("QUEUE", 1, None)], 100)
                .err(),
            Some(MqIntentProblem::Host(HostProblem::UnknownOutcome))
        );
        unchanged(&*store, &f, epoch);
    }
}
