//! Actual compiler/published installed child/core/CALL route. MQ and frame are
//! explicit fixtures: these tests claim no selected-service/SAF/UOW acceptance.
use super::*;
use mainframe_env_host_api::{MqHconn, mq_mqi::MqMqiUnitOfWork};

struct SyncProvider {
    inner: Arc<Provider>,
    unit: Arc<AtomicU64>,
    effects: Mutex<Vec<(Invocation, EffectRequest)>>,
}
impl HostProvider for SyncProvider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        self.inner.descriptor()
    }
    fn invoke(&self, invocation: &Invocation, effect: EffectRequest) -> EffectResult {
        self.effects
            .lock()
            .unwrap()
            .push((invocation.clone(), effect.clone()));
        let occurrence = effect
            .mq_mqi_occurrence(Default::default())
            .unwrap()
            .unwrap();
        let envelope = occurrence.envelope();
        let (connection, unit) = match envelope.request {
            MqMqiRequest::Commit { connection, unit } | MqMqiRequest::Back { connection, unit } => {
                (connection, unit)
            }
            _ => return self.inner.invoke(invocation, effect),
        };
        self.inner
            .registry
            .lock()
            .unwrap()
            .validate_connection(envelope.context.owner, connection)
            .unwrap();
        assert_eq!(unit, self.unit.load(Ordering::SeqCst));
        let core = self
            .inner
            .store
            .effect(effect.idempotency_key.as_ref().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(core.state, EffectState::Intent);
        assert_eq!(core.execution_id, invocation.execution_id);
        assert_eq!(
            core.request_digest,
            canonical_request_digest(&effect.request).unwrap()
        );
        self.inner
            .keys
            .lock()
            .unwrap()
            .push(effect.idempotency_key.clone().unwrap());
        // Fixture current-unit advance, not a product durable decision algorithm.
        self.unit.store(unit + 1, Ordering::SeqCst);
        EffectResult {
            sequence: effect.sequence,
            outcome: Ok(HostResult::MqMqi(Box::new(MqMqiHostResult {
                limits: envelope.limits,
                result: MqMqiResult {
                    call: envelope.request.call(),
                    outcome: MqMqiOutcome::Completed {
                        status: MqMqiStatus::OkNone,
                        output: MqMqiOutput::UnitOfWork { unit },
                    },
                },
            }))),
        }
    }
}
struct SyncFrame {
    invocation: Invocation,
    provider: Arc<Provider>,
    unit: Arc<AtomicU64>,
    lookups: Arc<Mutex<Vec<(Invocation, MqHconn, MqMqiUnitOfWork)>>>,
}
impl MqMqiProgramFrame for SyncFrame {
    fn profile(&self, invocation: &Invocation) -> Result<MqMqiProgramProfile, HostProblem> {
        Frame(self.invocation.clone()).profile(invocation)
    }
    fn local_unit(
        &self,
        invocation: &Invocation,
        connection: MqHconn,
    ) -> Result<MqMqiUnitOfWork, HostProblem> {
        let owner = self.profile(invocation)?.context.owner;
        self.provider
            .registry
            .lock()
            .unwrap()
            .validate_connection(owner, connection)
            .map_err(|_| HostProblem::Unauthorized)?;
        let unit = MqMqiUnitOfWork::Local {
            unit: self.unit.load(Ordering::SeqCst),
        };
        self.lookups
            .lock()
            .unwrap()
            .push((invocation.clone(), connection, unit));
        Ok(unit)
    }
}
#[test]
fn guarded_installed_connect_commit_back_disconnect_keep_original_effects_and_current_units() {
    for sqlite in [false, true] {
        let mut fixture = Fixture::new(sqlite);
        let unit = Arc::new(AtomicU64::new(31));
        let provider = Arc::new(SyncProvider {
            inner: fixture.provider.clone(),
            unit: unit.clone(),
            effects: Mutex::new(vec![]),
        });
        fixture.host = Arc::new(ScopedHostService::new(
            Arc::new(
                RegistrySnapshot::new(
                    1,
                    vec![
                        Arc::new(WeakProgramRouter::new(&fixture.router)) as Arc<dyn HostProvider>,
                        provider.clone(),
                    ],
                    InvocationLimits::default(),
                )
                .unwrap(),
            ),
            Default::default(),
        ));
        let observed = admission(false);
        let recorded = observed.clone();
        let lookup_provider = fixture.provider.clone();
        let lookups = Arc::new(Mutex::new(vec![]));
        let captured = lookups.clone();
        fixture.bind_source(
            Arc::new(Factory(Box::new(move |proof| {
                recorded
                    .observed
                    .lock()
                    .unwrap()
                    .push(proof.child().clone());
                Ok(Box::new(Session {
                    frame: Arc::new(SyncFrame {
                        invocation: proof.child().clone(),
                        provider: lookup_provider.clone(),
                        unit: unit.clone(),
                        lookups: captured.clone(),
                    }),
                    events: recorded.events.clone(),
                    aborts: recorded.aborts.clone(),
                    store: proof.store().clone(),
                    control: proof.execution_control().clone(),
                }))
            }))),
            None,
            &SOURCE.replace(
                "CALL 'MQDISC'",
                "CALL 'MQCMIT' USING HC CC RC. CALL 'MQBACK' USING HC CC RC. CALL 'MQDISC'",
            ),
        );
        let parent = fixture.parent();
        let original = fixture.effect(&parent, input());
        let (outcome, reply) = fixture.run_parent(parent.clone(), input());
        assert!(
            matches!(outcome, ExecutionOutcome::Completed(_)),
            "{outcome:?}"
        );
        assert!(matches!(reply.unwrap().outcome, Ok(HostResult::Program(_))));
        let child = observed.observed.lock().unwrap()[0].clone();
        assert_eq!(
            child.parent_execution_id.as_ref(),
            Some(&parent.execution_id)
        );
        let effects = provider.effects.lock().unwrap().clone();
        assert_eq!(effects.len(), 4);
        let lookups = lookups.lock().unwrap().clone();
        assert_eq!(lookups.len(), 2);
        assert_eq!(lookups[0].0, child);
        assert_eq!(lookups[1].0, child);
        assert_eq!(lookups[0].1, lookups[1].1);
        for (i, (invocation, effect)) in effects.iter().enumerate() {
            assert_eq!(invocation, &child);
            assert_eq!(effect.sequence, i as u64 + 1);
            assert_eq!(effect.run_unit, parent.run_unit_id);
            assert_eq!(effect.deadline_tick, parent.deadline_tick);
            let occurrence = effect
                .mq_mqi_occurrence(Default::default())
                .unwrap()
                .unwrap();
            let envelope = occurrence.envelope();
            assert_eq!(occurrence.mutation().sequence, effect.sequence);
            assert_eq!(
                Some(&occurrence.mutation().idempotency_key),
                effect.idempotency_key.as_ref()
            );
            match (i, &envelope.request) {
                (0, MqMqiRequest::Connect(_)) => {}
                (1, MqMqiRequest::Commit { connection, unit })
                | (2, MqMqiRequest::Back { connection, unit }) => {
                    assert_eq!(*connection, lookups[i - 1].1);
                    assert_eq!(MqMqiUnitOfWork::Local { unit: *unit }, lookups[i - 1].2);
                    assert_eq!(*unit, 30 + i as u64);
                }
                (3, MqMqiRequest::Disconnect { connection }) => {
                    assert_eq!(*connection, lookups[0].1)
                }
                _ => panic!("original effect order"),
            }
            let core = fixture
                .store
                .effect(effect.idempotency_key.as_ref().unwrap())
                .unwrap()
                .unwrap();
            assert_eq!(core.state, EffectState::Completed);
            assert_eq!(core.execution_id, child.execution_id);
            assert_eq!(
                core.request_digest,
                canonical_request_digest(&effect.request).unwrap()
            );
        }
        assert_eq!(observed.events.lock().unwrap().len(), 1);
        assert!(observed.aborts.lock().unwrap().is_empty());
        assert_eq!(
            fixture.provider.registry.lock().unwrap().active_handles(),
            0
        );
        assert_eq!(
            fixture
                .store
                .effect(original.idempotency_key.as_ref().unwrap())
                .unwrap()
                .unwrap()
                .request_digest,
            canonical_request_digest(&original.request).unwrap()
        );
        drop(provider);
        fixture.close_reopen();
    }
}

fn binding(schema: &str, bytes: &[u8]) -> BoundedPayload {
    BoundedPayload::new(schema, bytes.to_vec(), InvocationLimits::default()).unwrap()
}
#[test]
fn original_parent_bad_or_conflicting_mq_context_is_not_rewritten_or_admitted() {
    for sqlite in [false, true] {
        for (schema, bytes) in [
            ("wrong@1", b"zos-batch|queue-manager".as_slice()),
            (
                "mainframe-env.mq.host-context@1",
                b"not-a-context".as_slice(),
            ),
            (
                "mainframe-env.mq.host-context@1",
                b"mqi-client|queue-manager".as_slice(),
            ),
            (
                "mainframe-env.mq.host-context@1",
                b"zos-ims-batch-dli|queue-manager".as_slice(),
            ),
            (
                "mainframe-env.mq.host-context@1",
                b"zos-ims|host-coordinator".as_slice(),
            ),
            (
                "mainframe-env.mq.host-context@1",
                b"zos-batch|host-coordinator".as_slice(),
            ),
            (
                "mainframe-env.mq.host-context@1",
                b"zos-batch|queue-manager\0".as_slice(),
            ),
        ] {
            let mut fixture = Fixture::new(sqlite);
            let observed = admission(false);
            let original_observation = Arc::new(Mutex::new(None));
            let capture = original_observation.clone();
            let store = fixture.store.clone();
            fixture.bind(
                observed.clone(),
                Some(Box::new(move |parent, effect| {
                    *capture.lock().unwrap() = Some((
                        parent.clone(),
                        effect.clone(),
                        store
                            .effect(effect.idempotency_key.as_ref().unwrap())
                            .unwrap()
                            .unwrap(),
                    ));
                })),
            );
            let mut parent = fixture.parent();
            parent
                .bindings
                .insert("mq.host-context".into(), binding(schema, bytes));
            let before = parent.clone();
            let original = fixture.effect(&parent, input());
            let (_, reply) = fixture.run_parent(parent.clone(), input());
            assert_eq!(reply.unwrap().outcome, Err(HostProblem::Malformed));
            assert_eq!(parent, before);
            let (captured_parent, captured_effect, core_before) =
                original_observation.lock().unwrap().clone().unwrap();
            assert_eq!(captured_parent, before);
            assert_eq!(captured_effect, original);
            let core = fixture
                .store
                .effect(original.idempotency_key.as_ref().unwrap())
                .unwrap()
                .unwrap();
            assert_eq!(core.request_digest, core_before.request_digest);
            assert_eq!(core.execution_id, core_before.execution_id);
            assert_eq!(core.run_unit_id, core_before.run_unit_id);
            assert_eq!(core.sequence, core_before.sequence);
            assert_eq!(core.intent, core_before.intent);
            assert!(observed.observed.lock().unwrap().is_empty());
            assert!(observed.events.lock().unwrap().is_empty());
            assert!(observed.aborts.lock().unwrap().is_empty());
            fixture.assert_no_mq();
            let rows = fixture.pending();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].version, 1);
            let reservation: serde_json::Value = serde_json::from_slice(&rows[0].payload).unwrap();
            assert!(reservation["reply"].is_null());
            assert_eq!(reservation["owner_execution"], parent.execution_id.as_str());
            assert_eq!(reservation["owner_run_unit"], parent.run_unit_id.as_str());
            assert_eq!(
                reservation["owner_principal"],
                parent.principal.id().as_str()
            );
            assert!(crate::cobol::replay::describe_call_replay_row(&rows[0]).is_ok());
            fixture.close_reopen();
        }
    }
}
#[test]
fn matching_inherited_binding_is_preserved_and_only_absent_child_gets_trusted_batch_setup() {
    for sqlite in [false, true] {
        for present in [false, true] {
            let mut fixture = Fixture::new(sqlite);
            let observed = admission(false);
            let recorded = observed.clone();
            let expected = binding(
                "mainframe-env.mq.host-context@1",
                b"zos-batch|queue-manager",
            );
            let wanted = expected.clone();
            fixture.bind(
                Arc::new(Factory(Box::new(move |proof| {
                    assert_eq!(proof.child().bindings.get("mq.host-context"), Some(&wanted));
                    assert_eq!(
                        proof.parent().bindings.get("mq.host-context"),
                        present.then_some(&wanted)
                    );
                    assert_eq!(
                        proof.core_intent().request_digest,
                        canonical_request_digest(&proof.original_call().request).unwrap()
                    );
                    recorded
                        .observed
                        .lock()
                        .unwrap()
                        .push(proof.child().clone());
                    Ok(session(proof, &recorded))
                }))),
                None,
            );
            let mut parent = fixture.parent();
            if present {
                parent.bindings.insert("mq.host-context".into(), expected);
            }
            let original = parent.clone();
            let (_, reply) = fixture.run_parent(parent.clone(), input());
            assert!(matches!(reply.unwrap().outcome, Ok(HostResult::Program(_))));
            assert_eq!(parent, original);
            assert_eq!(fixture.provider.keys.lock().unwrap().len(), 2);
            assert_eq!(observed.events.lock().unwrap().len(), 1);
            assert!(observed.aborts.lock().unwrap().is_empty());
            fixture.close_reopen();
        }
    }
}
