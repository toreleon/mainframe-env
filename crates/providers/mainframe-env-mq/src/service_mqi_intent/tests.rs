use super::*;
use crate::mqi_admission::{MqMqiServiceScope, admit_mqi};
use mainframe_env_execution_api::*;
use mainframe_env_host_api::mq_mqi::*;
use mainframe_env_host_api::*;
use mainframe_env_store::{MemoryStore, SqliteStateStore, StoreLimits};
use mainframe_env_store_api::*;
use std::collections::{BTreeMap, BTreeSet};

struct Fixture {
    invocation: Invocation,
    effect: EffectRequest,
    owner: MqHandleOwner,
    provider: CapabilityDescriptor,
}
impl Fixture {
    fn new() -> Self {
        let l = InvocationLimits::default();
        let invocation = Invocation::new(
            RequestId::new("request", l).unwrap(),
            ExecutionId::new("execution", l).unwrap(),
            RunUnitId::new("run", l).unwrap(),
            None,
            Selector::new("test", l).unwrap(),
            ArtifactRef::new("artifact", l).unwrap(),
            Principal::new(
                PrincipalId::new("ISSUER", l).unwrap(),
                BTreeSet::from([CapabilityId::new("host.mq.write", l).unwrap()]),
                l,
            )
            .unwrap(),
            ServiceClass::System,
            0,
            100,
            TraceId::new("trace", l).unwrap(),
            IdempotencyKey::new("invocation-key", l).unwrap(),
            1,
            ResourceLimits::default(),
            BTreeMap::from([(
                "mq.host-context".into(),
                BoundedPayload::new(
                    "mainframe-env.mq.host-context@1",
                    b"other-bindings|queue-manager".to_vec(),
                    l,
                )
                .unwrap(),
            )]),
            l,
        )
        .unwrap()
        .with_cancellation_probe(CancellationProbe::new());
        let owner = MqHandleOwner {
            environment: MqHostEnvironment::OtherBindings,
            host_id: 1,
            process_id: 2,
            thread_id: 3,
            task_id: 4,
            syncpoint_epoch: 5,
        };
        let mutation = Mutation {
            sequence: 7,
            idempotency_key: IdempotencyKey::new("effect-key", l).unwrap(),
            transaction: None,
        };
        let effect = EffectRequest {
            run_unit: invocation.run_unit_id.clone(),
            sequence: 7,
            deadline_tick: 90,
            idempotency_key: Some(mutation.idempotency_key.clone()),
            request: HostRequest::MqMqi(MqMqiHostRequest {
                mutation,
                envelope: MqMqiRequestEnvelope {
                    context: MqMqiContext {
                        owner,
                        syncpoint_owner: MqSyncpointOwner::QueueManager,
                    },
                    limits: MqMqiLimits::default(),
                    request: MqMqiRequest::Connect(MqMqiConnect {
                        manager: None,
                        sharing: MqHandleSharing::NonShared,
                        options: MqMqiOptions::ContractDefault,
                    }),
                },
            }),
        };
        let provider = CapabilityDescriptor {
            capability: CapabilityId::new("host.mq.write", l).unwrap(),
            provider_id: "mainframe-env-mq".into(),
            generation: "1".into(),
            request_schema: "mainframe-env.mq-request@1".into(),
            result_schema: "mainframe-env.mq-result@1".into(),
            max_request_bytes: 4 << 20,
            max_result_bytes: 4 << 20,
            ready: true,
        };
        Self {
            invocation,
            effect,
            owner,
            provider,
        }
    }
    fn scope(&self) -> MqMqiServiceScope<'_> {
        MqMqiServiceScope::for_host_dispatch(
            &self.invocation,
            self.owner,
            self.effect
                .mq_mqi_occurrence(HostLimits::default())
                .unwrap()
                .unwrap(),
            &self.provider,
            HostLimits::default(),
        )
    }
    fn audit(&self, tick: u64) -> AuditRecord {
        AuditRecord {
            execution_id: self.invocation.execution_id.clone(),
            run_unit_id: self.invocation.run_unit_id.clone(),
            attempt: self.invocation.attempt,
            effect_sequence: self.effect.sequence,
            observed_tick: tick,
            principal: self.invocation.principal.id().clone(),
            invocation_key: self.invocation.idempotency_key.clone(),
            capability: self.provider.capability.clone(),
            resource: canonical_audit_resource_digest(&self.effect.request),
            decision: AuditDecision::Success,
        }
    }
    // Test-only coordinator-shaped fixture, never production observation authority.
    fn intent(&self) -> EffectRecord {
        EffectRecord {
            execution_id: self.invocation.execution_id.clone(),
            run_unit_id: self.invocation.run_unit_id.clone(),
            sequence: self.effect.sequence,
            key: self.effect.idempotency_key.clone().unwrap(),
            digest_format: EffectDigestFormat::CanonicalHostV1,
            request_digest: canonical_request_digest(&self.effect.request).unwrap(),
            state: EffectState::Intent,
            result_digest: None,
            resolved_tick: None,
            intent: EffectIntentMetadata {
                owner: self.invocation.execution_id.clone(),
                attempt: self.invocation.attempt,
                capability: Some(self.provider.capability.clone()),
                audit_resource: Some(self.audit(20).resource),
                audit_invocation_key: Some(self.invocation.idempotency_key.clone()),
                created_tick: 8,
                recovery_after_tick: self.invocation.deadline_tick.min(self.effect.deadline_tick),
                epoch: 8,
                recovery_lease: None,
            },
        }
    }
    fn execution(&self) -> ExecutionRecord {
        ExecutionRecord {
            execution_id: self.invocation.execution_id.clone(),
            run_unit_id: self.invocation.run_unit_id.clone(),
            selector: self.invocation.selector.clone(),
            artifact: self.invocation.artifact.clone(),
            principal: self.invocation.principal.id().clone(),
            state: ExecutionState::Admitted,
            attempt: self.invocation.attempt,
            version: 1,
            owner_lease: None,
            lease_expiry_tick: None,
            terminal_tick: None,
        }
    }
    fn seed_execution(&self, store: &dyn PlatformStore, execution: ExecutionRecord) {
        store.create_execution(execution).unwrap();
        store
            .transition_execution(&self.invocation.execution_id, 1, ExecutionState::Queued, 6)
            .unwrap();
        store
            .transition_execution(&self.invocation.execution_id, 2, ExecutionState::Running, 7)
            .unwrap();
    }
    fn seed(&self, store: &dyn PlatformStore) {
        self.seed_execution(store, self.execution());
        store.record_intent(self.intent()).unwrap();
    }
}

fn backends() -> Vec<Box<dyn PlatformStore>> {
    vec![
        Box::new(MemoryStore::new(StoreLimits {
            max_blob_bytes: 8192,
            ..StoreLimits::default()
        })),
        Box::new(SqliteStateStore::open("sqlite::memory:", 8192, 64).unwrap()),
    ]
}
fn queue(key: &str, version: u64, expected: Option<u64>) -> ProviderStateMutation {
    let payload = crate::service::encode_object_row(
        key,
        &serde_json::json!({
            "trigger_program": null, "messages": []
        }),
    )
    .unwrap();
    ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: "mq-v1-queue".into(),
            key: key.into(),
            version,
            payload,
        },
        expected_version: expected,
    })
}
fn unchanged(store: &dyn PlatformStore, fixture: &Fixture, epoch: u64) {
    assert!(
        store
            .list_provider_state("mq-v1-queue", 1)
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .audit_records(&fixture.invocation.execution_id, 1, 1)
            .unwrap()
            .is_empty()
    );
    assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
}

#[test]
fn actual_original_admission_publishes_same_store_rows_and_audit_without_core_completion() {
    for store in backends() {
        let f = Fixture::new();
        f.seed(&*store);
        let execution = store.get_execution(&f.invocation.execution_id).unwrap();
        let initial_epoch = store.provider_state_retention_epoch().unwrap();
        let scope = f.scope();
        let admission = admit_mqi(&scope, &f.invocation, 10).unwrap();
        let mut bound = bind_core_intent(&admission, &*store, 20).unwrap();
        bound
            .prepare(f.audit(21), vec![queue("QUEUE", 1, None)], 21)
            .unwrap()
            .publish(21)
            .unwrap();
        assert_eq!(store.effect(&f.intent().key).unwrap(), Some(f.intent()));
        assert_eq!(
            store.get_execution(&f.invocation.execution_id).unwrap(),
            execution
        );
        assert_eq!(
            store
                .audit_records(&f.invocation.execution_id, 1, 8)
                .unwrap(),
            vec![f.audit(21)]
        );
        assert_eq!(
            store
                .get_provider_state("mq-v1-queue", "QUEUE")
                .unwrap()
                .unwrap()
                .version,
            1
        );
        assert_eq!(
            store.provider_state_retention_epoch().unwrap(),
            initial_epoch + 2
        );
        // At-most-once depends on the caller's actual CAS, not this binding.
        let epoch = store.provider_state_retention_epoch().unwrap();
        assert!(
            bound
                .prepare(f.audit(22), vec![queue("QUEUE", 1, None)], 22)
                .unwrap()
                .publish(22)
                .is_err()
        );
        assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
        assert_eq!(
            store
                .audit_records(&f.invocation.execution_id, 1, 8)
                .unwrap()
                .len(),
            1
        );
    }
}

#[test]
fn retained_intent_identity_domain_metadata_and_deadline_conflicts_fail_without_publication() {
    for case in 0..23 {
        for store in backends() {
            let f = Fixture::new();
            f.seed_execution(&*store, f.execution());
            let mut record = f.intent();
            match case {
                0 => record.sequence += 1,
                1 => record.request_digest[0] ^= 1,
                2 => record.digest_format = EffectDigestFormat::LegacyDebug,
                3 => record.intent.attempt += 1,
                4 => {
                    record.intent.capability = Some(
                        CapabilityId::new("host.mq.read", InvocationLimits::default()).unwrap(),
                    )
                }
                5 => record.intent.audit_resource = None,
                6 => record.intent.audit_resource.as_mut().unwrap().value[0] ^= 1,
                7 => {
                    record.intent.audit_resource.as_mut().unwrap().format =
                        AuditResourceDigestFormat::CanonicalHostOversizedResourceV1
                }
                8 => record.intent.audit_invocation_key = None,
                9 => {
                    record.intent.audit_invocation_key =
                        Some(IdempotencyKey::new("foreign", InvocationLimits::default()).unwrap())
                }
                10 => record.intent.created_tick = 0,
                11 => record.intent.created_tick = 21,
                12 => record.intent.recovery_after_tick = 89,
                13 => record.intent.recovery_after_tick = 91,
                14 => {
                    record.intent.recovery_lease = Some(EffectRecoveryLease {
                        owner: "recovery".into(),
                        attempt: 1,
                        epoch: 9,
                        expires_tick: 100,
                    })
                }
                15 => {
                    record.key =
                        IdempotencyKey::new("foreign", InvocationLimits::default()).unwrap()
                }
                16 => record.intent.recovery_after_tick = u64::MAX,
                17 => record.intent.capability = None,
                18 => record.intent.epoch = 0,
                19 => {
                    record.intent.owner =
                        ExecutionId::new("foreign", InvocationLimits::default()).unwrap()
                }
                20 => record.result_digest = Some([1; 32]),
                21 => record.resolved_tick = Some(20),
                22 => record.intent.attempt = 0,
                _ => unreachable!(),
            }
            // Some coherent foreign attempts are rejected by the core writer
            // itself; do not weaken that authority to manufacture bad fixtures.
            let seeded = store.record_intent(record.clone()).is_ok();
            let epoch = store.provider_state_retention_epoch().unwrap();
            let scope = f.scope();
            let admission = admit_mqi(&scope, &f.invocation, 10).unwrap();
            assert!(
                bind_core_intent(&admission, &*store, 20).is_err(),
                "case {case}, seeded {seeded}"
            );
            unchanged(&*store, &f, epoch);
        }
    }
}

#[test]
fn original_invocation_and_host_payload_are_not_replaceable_by_an_existing_intent() {
    for case in 0..8 {
        for store in backends() {
            let mut f = Fixture::new();
            f.seed(&*store);
            match case {
                0 => {
                    f.invocation.execution_id =
                        ExecutionId::new("other", InvocationLimits::default()).unwrap()
                }
                1 => {
                    f.invocation.run_unit_id =
                        RunUnitId::new("other", InvocationLimits::default()).unwrap();
                    f.effect.run_unit = f.invocation.run_unit_id.clone();
                }
                2 => f.invocation.attempt += 1,
                3 => {
                    f.invocation.principal = Principal::new(
                        PrincipalId::new("OTHER", InvocationLimits::default()).unwrap(),
                        f.invocation.principal.grants().clone(),
                        InvocationLimits::default(),
                    )
                    .unwrap()
                }
                4 => {
                    f.invocation.idempotency_key =
                        IdempotencyKey::new("other", InvocationLimits::default()).unwrap()
                }
                5 => {
                    f.invocation.selector =
                        Selector::new("other", InvocationLimits::default()).unwrap()
                }
                6 => {
                    f.invocation.artifact =
                        ArtifactRef::new("other", InvocationLimits::default()).unwrap()
                }
                7 => {
                    if let HostRequest::MqMqi(request) = &mut f.effect.request {
                        request.mutation.transaction = Some("another-unit".into());
                    }
                }
                _ => unreachable!(),
            }
            let epoch = store.provider_state_retention_epoch().unwrap();
            let scope = f.scope();
            let admission = admit_mqi(&scope, &f.invocation, 10).unwrap();
            assert!(
                bind_core_intent(&admission, &*store, 20).is_err(),
                "case {case}"
            );
            assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
            assert!(
                store
                    .list_provider_state("mq-v1-queue", 8)
                    .unwrap()
                    .is_empty()
            );
        }
    }
}

mod controls;

#[test]
fn admission_summary_substitution_cannot_replace_the_immutable_original_effect() {
    for case in 0..6 {
        for store in backends() {
            let f = Fixture::new();
            f.seed_execution(&*store, f.execution());
            let mut record = f.intent();
            if case == 0 {
                record.request_digest[0] ^= 1;
            }
            if case == 1 {
                record.intent.capability =
                    Some(CapabilityId::new("host.mq.read", InvocationLimits::default()).unwrap());
            }
            store.record_intent(record.clone()).unwrap();
            let HostRequest::MqMqi(mut alternate) = f.effect.request.clone() else {
                unreachable!()
            };
            alternate.mutation.transaction = Some("substituted-unit".into());
            if let MqMqiRequest::Connect(connect) = &mut alternate.envelope.request {
                connect.manager = Some(
                    mainframe_env_host_api::mq_object_route::MqRouteName::new("OTHER").unwrap(),
                );
            }
            let scope = f.scope();
            let mut admission = admit_mqi(&scope, &f.invocation, 10).unwrap();
            let MqMqiAdmission::ServiceValidation(identity) = &mut admission else {
                unreachable!()
            };
            match case {
                0 => identity.host_request_digest = record.request_digest,
                1 => identity.capability = record.intent.capability.clone().unwrap(),
                2 => identity.mutation = &alternate.mutation,
                3 => identity.envelope = &alternate.envelope,
                4 => identity.owner.task_id += 1,
                5 => identity.origin = crate::retention::MqReplayOwnerKind::CicsNested,
                _ => unreachable!(),
            }
            let epoch = store.provider_state_retention_epoch().unwrap();
            assert!(
                bind_core_intent(&admission, &*store, 20).is_err(),
                "summary {case}"
            );
            unchanged(&*store, &f, epoch);
            assert_eq!(store.effect(&record.key).unwrap(), Some(record));
        }
    }
}
