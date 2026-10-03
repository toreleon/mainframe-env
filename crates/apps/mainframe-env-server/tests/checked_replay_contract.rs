//! Genuine coordinator regression using fixture-owned structural observations.
//! These receivers, tokens and receipt do not attest an installed MQINQ provider,
//! live selected frame, SAF, native ABI, JES or supervised root authority.
#[cfg(test)]
mod tests {
    use mainframe_env_execution_api::*;
    use mainframe_env_host_api::mq_mqi::*;
    use mainframe_env_host_api::*;
    use mainframe_env_interpreter::*;
    use mainframe_env_store::*;
    use mainframe_env_store_api::*;
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicU8, AtomicUsize, Ordering},
    };
    struct Receiver {
        descriptor: CapabilityDescriptor,
        store: Arc<dyn PlatformStore>,
        mode: AtomicU8,
        invokes: AtomicUsize,
        replays: AtomicUsize,
        stored: Mutex<Option<EffectResult>>,
        before: Mutex<Option<Snapshot>>,
        captured_execution: Mutex<Option<ExecutionRecord>>,
    }
    impl HostProvider for Receiver {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.descriptor
        }
        fn invoke(&self, i: &Invocation, r: EffectRequest) -> EffectResult {
            self.invokes.fetch_add(1, Ordering::SeqCst);
            if let Some(prior) = self.stored.lock().unwrap().as_ref() {
                return prior.clone();
            }
            let q = match &r.request {
                HostRequest::MqMqi(h) => match &h.envelope.request {
                    MqMqiRequest::Inquire(q) => q,
                    _ => panic!(),
                },
                _ => panic!(),
            };
            let limits = match &r.request {
                HostRequest::MqMqi(h) => h.envelope.limits,
                _ => panic!(),
            };
            let result = EffectResult {
                sequence: r.sequence,
                outcome: Ok(HostResult::MqMqi(MqMqiHostResult {
                    limits,
                    result: MqMqiResult {
                        call: MqMqiCall::Inquire,
                        outcome: MqMqiOutcome::Completed {
                            status: MqMqiStatus::OkNone,
                            output: MqMqiOutput::Attributes {
                                integers: vec![1; q.selectors.len()],
                                characters: vec![],
                            },
                        },
                    },
                })),
            };
            // A fixture receipt is structural input; original core/audit are genuinely
            // created by OriginalDispatch, not seeded Completed/Running permission.
            assert_eq!(
                self.store
                    .effect(r.idempotency_key.as_ref().unwrap())
                    .unwrap()
                    .unwrap()
                    .state,
                EffectState::Intent
            );
            self.store
                .put_provider_state(
                    ProviderStateRecord {
                        namespace: "proof-observation-v1".into(),
                        key: "receipt".into(),
                        version: 1,
                        payload: b"exact fixture prior result".to_vec(),
                    },
                    None,
                )
                .unwrap();
            *self.stored.lock().unwrap() = Some(result.clone());
            assert_eq!(r.run_unit, i.run_unit_id);
            result
        }
        fn replay_retained(
            &self,
            i: &Invocation,
            r: EffectRequest,
            digest: [u8; 32],
            tick: u64,
            context: &(dyn std::any::Any + Send + Sync),
        ) -> EffectResult {
            self.replays.fetch_add(1, Ordering::SeqCst);
            let c = context.downcast_ref::<CheckedReplayAuditCapture>().unwrap();
            assert_eq!(c.invocation(), i);
            assert_eq!(c.observed_tick(), tick);
            assert_eq!(c.effect().result_digest, Some(digest));
            assert_eq!(
                c.execution(),
                &self.store.get_execution(&i.execution_id).unwrap().unwrap()
            );
            *self.captured_execution.lock().unwrap() = Some(c.execution().clone());
            let row = self
                .store
                .get_provider_state("proof-observation-v1", "receipt")
                .unwrap()
                .unwrap();
            let observations = || CheckedReplayObservations {
                receipt: Some(ProviderStateIdentity {
                    namespace: row.namespace.clone(),
                    key: row.key.clone(),
                }),
                dependencies: vec![TerminalRowDependency::Exact(row.clone())],
            };
            let mode = self.mode.load(Ordering::SeqCst);
            if mode == 6 {
                let foreign: Arc<dyn PlatformStore> =
                    Arc::new(MemoryStore::new(StoreLimits::default()));
                assert!(c.submit(&foreign, observations()).is_err());
            } else if mode != 7 {
                c.submit(&self.store, observations()).unwrap();
                assert!(c.submit(&self.store, observations()).is_err());
            }
            *self.before.lock().unwrap() = Some(snapshot(&self.store, i, &r));
            if mode == 8 {
                panic!("actual scoped replay panic");
            }
            if mode == 9 {
                self.store
                    .put_provider_state(ProviderStateRecord { version: 2, ..row }, Some(1))
                    .unwrap();
            }
            if mode == 10 {
                self.store
                    .transition_execution(
                        &i.execution_id,
                        c.execution().version,
                        ExecutionState::Failed,
                        tick,
                    )
                    .unwrap();
            }
            let mut result = self.stored.lock().unwrap().clone().unwrap();
            result.sequence = r.sequence;
            result.outcome = match mode {
                1 => Err(HostProblem::Unauthorized),
                2 => Err(HostProblem::Malformed),
                3 => Err(HostProblem::InfrastructureFailure),
                4 => Ok(HostResult::State {
                    value: None,
                    version: 0,
                }),
                _ => result.outcome,
            };
            if mode == 11 {
                result.sequence += 1;
                result.outcome = Err(HostProblem::Unauthorized);
            }
            result
        }
    }
    struct Emission {
        request: EffectRequest,
        invocation: Invocation,
        received: Option<EffectResult>,
        store: Arc<dyn PlatformStore>,
        receiver: Arc<Receiver>,
        check_replay: bool,
    }
    impl Machine for Emission {
        type Effect = EffectRequest;
        type EffectResult = EffectResult;
        fn drive(
            &mut self,
            r: MachineResume<EffectResult>,
            _: Quantum,
        ) -> MachineDrive<EffectRequest> {
            match r {
                MachineResume::Start => MachineDrive::HostCall(self.request.clone()),
                MachineResume::HostResult(result) => {
                    if self.check_replay && result.outcome.is_ok() {
                        let before = self.receiver.before.lock().unwrap();
                        let b = before.as_ref().unwrap();
                        // Compare before the machine's subsequent suspension
                        // lifecycle publication, which is a separate operation.
                        assert_eq!(snapshot(&self.store, &self.invocation, &self.request), *b);
                    }
                    self.received = Some(result);
                    MachineDrive::Suspended(Suspension {
                        kind: "fixture".into(),
                        resume_token: "fixture".into(),
                        state_bytes: 1,
                    })
                }
                _ => panic!(),
            }
        }
        fn checkpoint(&self) -> Option<BoundedPayload> {
            Some(BoundedPayload::new("fixture@1", vec![1], InvocationLimits::default()).unwrap())
        }
        fn effect_sequence(&self) -> u64 {
            1
        }
    }
    fn invocation() -> Invocation {
        let l = InvocationLimits::default();
        Invocation::new(
            RequestId::new("request", l).unwrap(),
            ExecutionId::new("execution", l).unwrap(),
            RunUnitId::new("run", l).unwrap(),
            None,
            Selector::new("fixture", l).unwrap(),
            ArtifactRef::new("artifact", l).unwrap(),
            Principal::new(
                PrincipalId::new("USER", l).unwrap(),
                BTreeSet::from([CapabilityId::new("host.mq.write", l).unwrap()]),
                l,
            )
            .unwrap(),
            ServiceClass::Batch,
            0,
            100,
            TraceId::new("trace", l).unwrap(),
            IdempotencyKey::new("invocation", l).unwrap(),
            1,
            ResourceLimits::default(),
            BTreeMap::new(),
            l,
        )
        .unwrap()
    }
    fn request(i: &Invocation, count: usize) -> EffectRequest {
        let owner = MqHandleOwner {
            environment: MqHostEnvironment::ZosBatch,
            host_id: 1,
            process_id: 1,
            thread_id: 1,
            task_id: 1,
            syncpoint_epoch: 1,
        };
        let mut registry = MqHandleRegistry::new(1, 10).unwrap();
        let h = registry.connect(owner, MqHandleSharing::NonShared).unwrap();
        let o = registry.create_object(owner, h).unwrap();
        let limits = MqMqiLimits::default();
        let q = MqMqiLocalTypeInquiry::new(h, o, &vec![20; count], count as i64 + 1, 0, limits)
            .unwrap();
        let key = IdempotencyKey::new("effect", InvocationLimits::default()).unwrap();
        EffectRequest {
            run_unit: i.run_unit_id.clone(),
            sequence: 1,
            deadline_tick: 100,
            idempotency_key: Some(key.clone()),
            request: HostRequest::MqMqi(MqMqiHostRequest {
                envelope: MqMqiRequestEnvelope {
                    context: MqMqiContext {
                        owner,
                        syncpoint_owner: MqSyncpointOwner::QueueManager,
                    },
                    limits,
                    request: MqMqiRequest::Inquire(q.into_inquiry()),
                },
                mutation: Mutation {
                    sequence: 1,
                    idempotency_key: key,
                    transaction: None,
                },
            }),
        }
    }
    fn exercise(store: Arc<dyn PlatformStore>, mode: u8, count: usize, legacy: bool) {
        let i = invocation();
        let r = request(&i, count);
        let l = InvocationLimits::default();
        let receiver = Arc::new(Receiver {
            descriptor: CapabilityDescriptor {
                capability: CapabilityId::new("host.mq.write", l).unwrap(),
                provider_id: "fixture".into(),
                generation: "1".into(),
                request_schema: "fixture@1".into(),
                result_schema: "fixture@1".into(),
                max_request_bytes: 8 * 1024 * 1024,
                max_result_bytes: 8 * 1024 * 1024,
                ready: true,
            },
            store: store.clone(),
            mode: AtomicU8::new(0),
            invokes: AtomicUsize::new(0),
            replays: AtomicUsize::new(0),
            stored: Mutex::new(None),
            before: Mutex::new(None),
            captured_execution: Mutex::new(None),
        });
        let host = Arc::new(ScopedHostService::new(
            Arc::new(RegistrySnapshot::new(1, vec![receiver.clone()], l).unwrap()),
            HostLimits::default(),
        ));
        let coordinator =
            ExecutionCoordinator::durable(host, store.clone(), CoordinatorLimits::default());
        let coordinator = if legacy {
            coordinator
        } else {
            coordinator.with_checked_inquiry_replay()
        };
        let mut first = Emission {
            request: r.clone(),
            invocation: i.clone(),
            received: None,
            store: store.clone(),
            receiver: receiver.clone(),
            check_replay: false,
        };
        assert!(matches!(
            coordinator.execute(
                &mut first,
                &i,
                ExecutionControl {
                    now_tick: 5,
                    cancellation_requested: false
                }
            ),
            ExecutionOutcome::Suspended(_)
        ));
        let completed = store
            .effect(r.idempotency_key.as_ref().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(completed.state, EffectState::Completed);
        assert_eq!(
            completed.request_digest,
            canonical_request_digest(&r.request).unwrap()
        );
        assert_eq!(completed.execution_id, i.execution_id);
        assert_eq!(completed.run_unit_id, i.run_unit_id);
        assert_eq!(completed.sequence, 1);
        assert_eq!(completed.key, *r.idempotency_key.as_ref().unwrap());
        assert_eq!(completed.resolved_tick, Some(5));
        assert_eq!(
            completed.result_digest,
            Some(canonical_result_digest(&first.received.as_ref().unwrap().outcome).unwrap())
        );
        let intent = &completed.intent;
        assert_eq!(intent.owner, i.execution_id);
        assert_eq!(intent.attempt, 1);
        assert_eq!(
            intent.capability,
            Some(CapabilityId::new("host.mq.write", l).unwrap())
        );
        assert_eq!(
            intent.audit_resource,
            Some(canonical_audit_resource_digest(&r.request))
        );
        assert_eq!(intent.audit_invocation_key, Some(i.idempotency_key.clone()));
        assert_eq!(intent.created_tick, 5);
        // OriginalDispatch uses the actual persisted cursor version, not an
        // arbitrary fixture epoch. Admitted/Started/Intent have advanced it.
        assert_eq!(intent.epoch, 4);
        assert!(intent.recovery_lease.is_none());
        let original_audits = store.audit_records(&i.execution_id, 1, 100).unwrap();
        assert_eq!(original_audits.len(), 1);
        assert_eq!(
            original_audits[0],
            expected_audit(&i, &r, 5, AuditDecision::Success)
        );
        receiver.mode.store(mode, Ordering::SeqCst);
        let mut second = Emission {
            request: r.clone(),
            invocation: i.clone(),
            received: None,
            store: store.clone(),
            receiver: receiver.clone(),
            check_replay: !legacy,
        };
        let result = coordinator.execute_resumable_with_control(&mut second, &i, || {
            Ok(ExecutionControl {
                now_tick: 10,
                cancellation_requested: false,
            })
        });
        assert_eq!(
            store.effect(r.idempotency_key.as_ref().unwrap()).unwrap(),
            Some(completed)
        );
        let audits = store.audit_records(&i.execution_id, 1, 100).unwrap();
        if legacy {
            assert_eq!(receiver.invokes.load(Ordering::SeqCst), 2);
            assert_eq!(receiver.replays.load(Ordering::SeqCst), 0);
            assert_eq!(audits.len(), 2);
            assert!(matches!(result, ExecutionOutcome::Suspended(_)));
            assert_eq!(second.received, first.received);
            assert_eq!(
                audits[1],
                expected_audit(&i, &r, 10, AuditDecision::Success)
            );
            assert!(coordinator.pending_checked_replay().is_none());
            return;
        }
        assert_eq!(receiver.invokes.load(Ordering::SeqCst), 1);
        assert_eq!(receiver.replays.load(Ordering::SeqCst), 1);
        if mode == 0 {
            assert!(matches!(result, ExecutionOutcome::Suspended(_)));
            assert_eq!(second.received, first.received);
            assert_eq!(audits, original_audits);
            assert!(coordinator.pending_checked_replay().is_none());
        } else if [1, 2, 3, 4, 8, 11].contains(&mode) {
            assert_eq!(audits.len(), 2);
            let a = &audits[1];
            assert_eq!(a.principal, *i.principal.id());
            assert_eq!(a.invocation_key, i.idempotency_key);
            assert_eq!(a.resource, canonical_audit_resource_digest(&r.request));
            assert_eq!(a.observed_tick, 10);
            assert_eq!(a.effect_sequence, 1);
            assert_eq!(
                a.decision,
                match mode {
                    1 => AuditDecision::Deny,
                    2 => AuditDecision::Rejected,
                    3 | 8 => AuditDecision::InfrastructureFailure,
                    _ => AuditDecision::UnknownOutcome,
                }
            );
            assert_eq!(*a, expected_audit(&i, &r, 10, a.decision));
            let events = store.events(&i.execution_id, 1, 100).unwrap();
            let captured = receiver.captured_execution.lock().unwrap().clone().unwrap();
            let expected = LifecycleEvent {
                execution_id: i.execution_id.clone(),
                run_unit_id: i.run_unit_id.clone(),
                sequence: captured.version + 1,
                attempt: 1,
                tick: 10,
                kind: LifecycleEventKind::EffectResult { sequence: 1 },
            };
            assert_eq!(
                events
                    .iter()
                    .filter(|e| e.kind == expected.kind && e.tick == 10)
                    .collect::<Vec<_>>(),
                vec![&expected]
            );
            let expected_notification = OutboxRecord {
                notification_id: format!("execution:{:020}", expected.sequence),
                execution_id: i.execution_id.clone(),
                sequence: expected.sequence,
                topic: "execution.lifecycle.v1".into(),
                payload: b"mainframe-env.execution-lifecycle@1\0\x07\0\0\0\0\0\0\0\x01".to_vec(),
                attempt: 0,
                delivered: false,
                delivered_tick: None,
                version: 1,
            };
            let outbox = store.pending_notifications(100).unwrap();
            assert_eq!(
                outbox
                    .iter()
                    .filter(|n| n.sequence == expected.sequence)
                    .collect::<Vec<_>>(),
                vec![&expected_notification]
            );
            if ![4, 11].contains(&mode) {
                assert!(matches!(result, ExecutionOutcome::Suspended(_)));
                let problem = match mode {
                    1 => HostProblem::Unauthorized,
                    2 => HostProblem::Malformed,
                    _ => HostProblem::InfrastructureFailure,
                };
                assert_eq!(
                    second.received,
                    Some(EffectResult {
                        sequence: 1,
                        outcome: Err(problem)
                    })
                );
            }
            if mode == 4 || mode == 11 {
                assert!(coordinator.pending_checked_replay().is_some());
            } else {
                assert!(coordinator.pending_checked_replay().is_none());
            }
        } else {
            assert!(coordinator.pending_checked_replay().is_some());
            assert_eq!(audits, original_audits);
            assert!(second.received.is_none());
        }
        if mode != 0 && ![1, 2, 3, 8].contains(&mode) {
            assert!(
                matches!(&result,ExecutionOutcome::ProviderFailure(p) if p.has_unknown_outcome())
            );
            assert!(second.received.is_none());
        }
        if let Some(c) = coordinator.pending_checked_replay() {
            assert_eq!(c.effect().state, EffectState::Completed);
            assert!(c.retained_audit().is_some());
            assert!(
                c.submit(
                    &store,
                    CheckedReplayObservations {
                        receipt: None,
                        dependencies: vec![]
                    }
                )
                .is_err()
            );
            let before = snapshot(&store, &i, &r);
            let refused = coordinator.execute_resumable_with_control(&mut second, &i, || {
                panic!("retained attempt must refuse before callback")
            });
            assert!(
                matches!(refused,ExecutionOutcome::ProviderFailure(p) if p.has_unknown_outcome())
            );
            assert_eq!(snapshot(&store, &i, &r), before);
            assert_eq!(receiver.replays.load(Ordering::SeqCst), 1);
        }
    }
    fn expected_audit(
        i: &Invocation,
        r: &EffectRequest,
        tick: u64,
        decision: AuditDecision,
    ) -> AuditRecord {
        AuditRecord {
            execution_id: i.execution_id.clone(),
            run_unit_id: i.run_unit_id.clone(),
            attempt: 1,
            effect_sequence: 1,
            observed_tick: tick,
            principal: i.principal.id().clone(),
            invocation_key: i.idempotency_key.clone(),
            capability: CapabilityId::new("host.mq.write", InvocationLimits::default()).unwrap(),
            resource: canonical_audit_resource_digest(&r.request),
            decision,
        }
    }
    #[derive(Debug, PartialEq)]
    struct Snapshot {
        epoch: u64,
        execution: Option<ExecutionRecord>,
        effect: Option<EffectRecord>,
        receipt: Option<ProviderStateRecord>,
        logical_clock: Option<ProviderStateRecord>,
        audits: Vec<AuditRecord>,
        events: Vec<LifecycleEvent>,
        outbox: Vec<OutboxRecord>,
    }
    fn snapshot(store: &Arc<dyn PlatformStore>, i: &Invocation, r: &EffectRequest) -> Snapshot {
        Snapshot {
            epoch: store.provider_state_retention_epoch().unwrap(),
            execution: store.get_execution(&i.execution_id).unwrap(),
            effect: store.effect(r.idempotency_key.as_ref().unwrap()).unwrap(),
            receipt: store
                .get_provider_state("proof-observation-v1", "receipt")
                .unwrap(),
            logical_clock: store
                .get_provider_state("jes-worker-meta", "logical-clock")
                .unwrap(),
            audits: store.audit_records(&i.execution_id, 1, 100).unwrap(),
            events: store.events(&i.execution_id, 1, 100).unwrap(),
            outbox: store.pending_notifications(100).unwrap(),
        }
    }
    struct Owned(std::path::PathBuf);
    impl Drop for Owned {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn actual_original_dispatch_completed_replay_memory_and_owned_sqlite() {
        for mode in [0, 1, 2, 3, 4, 6, 7, 8, 9, 10, 11] {
            for count in [0, 3] {
                exercise(
                    Arc::new(MemoryStore::new(StoreLimits::default())),
                    mode,
                    count,
                    false,
                );
                let path = std::env::temp_dir().join(format!(
                    "checked-replay-contract-{}-{mode}-{count}",
                    std::process::id()
                ));
                std::fs::create_dir(&path).unwrap();
                let owned = Owned(path);
                exercise(
                    Arc::new(
                        SqliteStateStore::open(
                            &format!("sqlite://{}/state.db?mode=rwc", owned.0.display()),
                            65536,
                            1000,
                        )
                        .unwrap(),
                    ),
                    mode,
                    count,
                    false,
                );
            }
        }
    }
    #[test]
    fn legacy_ordinary_completed_replay_still_invokes_and_audits() {
        exercise(
            Arc::new(MemoryStore::new(StoreLimits::default())),
            0,
            3,
            true,
        );
        let path = std::env::temp_dir().join(format!(
            "checked-replay-contract-legacy-{}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        let owned = Owned(path);
        exercise(
            Arc::new(
                SqliteStateStore::open(
                    &format!("sqlite://{}/state.db?mode=rwc", owned.0.display()),
                    65536,
                    1000,
                )
                .unwrap(),
            ),
            0,
            3,
            true,
        );
    }
}
