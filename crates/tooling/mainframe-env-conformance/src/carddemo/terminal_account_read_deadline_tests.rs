//! Actual Completed READ occurrence binding under a fresh online request budget.
use mainframe_env_cics::{CicsLimits, CicsReplayClock, CicsService, cics_provider};
use mainframe_env_dataset::{DatasetLimits, DatasetService, dataset_providers};
use mainframe_env_execution_api::*;
use mainframe_env_host_api::*;
use mainframe_env_interpreter::{
    CoordinatorLimits, DurableCompletedPlainRead, DurableEffectDispatch, DurableEffectDispatchHook,
    ExecutionControl, ExecutionCoordinator, ReferenceMachine,
};
use mainframe_env_store::MemoryStore;
use mainframe_env_store_api::*;
use std::collections::{BTreeMap, BTreeSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

struct Clock(AtomicU64);
impl CicsReplayClock for Clock {
    fn now_tick(&self) -> Result<u64, HostProblem> {
        match self.0.load(Ordering::SeqCst) {
            u64::MAX => Err(HostProblem::InfrastructureFailure),
            tick => Ok(tick),
        }
    }
}

struct Security {
    descriptor: CapabilityDescriptor,
    deny: Arc<AtomicBool>,
    store: Arc<MemoryStore>,
    clock: Arc<Clock>,
    armed: Arc<AtomicBool>,
    mode: &'static str,
    cancellation: CancellationProbe,
}
impl HostProvider for Security {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }
    fn invoke(&self, _: &Invocation, effect: EffectRequest) -> EffectResult {
        assert!(matches!(effect.request, HostRequest::Security(_)));
        if self.armed.load(Ordering::SeqCst) {
            // A real provider clock write reproduces SAF moving the atomic replay floor.
            self.store.advance_logical_clock(160).unwrap();
            let tick = match self.mode {
                "post-SAF-clock-regressed" => 140,
                "post-SAF-clock-below-store-floor" => 155,
                "post-SAF-clock-zero" => 0,
                "post-SAF-clock-unavailable" => u64::MAX,
                "post-SAF-current-expired" => 200,
                "post-SAF-session-expired" => 171,
                _ => 160,
            };
            self.clock.0.store(tick, Ordering::SeqCst);
            if self.mode == "post-SAF-cancelled" {
                self.cancellation.request();
            }
        }
        EffectResult {
            sequence: effect.sequence,
            outcome: Ok(HostResult::Security(if self.deny.load(Ordering::SeqCst) {
                SecurityDecision::Deny
            } else {
                SecurityDecision::Allow
            })),
        }
    }
}

// Observes the real provider; it never replaces a dataset reply or store method.
struct DatasetReadCount {
    inner: Arc<dyn HostProvider>,
    reads: Arc<AtomicUsize>,
}
impl HostProvider for DatasetReadCount {
    fn descriptor(&self) -> &CapabilityDescriptor {
        self.inner.descriptor()
    }
    fn invoke(&self, actor: &Invocation, effect: EffectRequest) -> EffectResult {
        if matches!(
            effect.request,
            HostRequest::Dataset(DatasetRequest::Read { .. })
        ) {
            self.reads.fetch_add(1, Ordering::SeqCst);
        }
        self.inner.invoke(actor, effect)
    }
}

struct Replay {
    cics: Arc<CicsService>,
    store: Arc<dyn PlatformStore>,
    mode: &'static str,
    calls: usize,
}
impl DurableEffectDispatchHook for Replay {
    fn with_dispatch(
        &mut self,
        actual: &DurableEffectDispatch<'_>,
        dispatch: &mut dyn FnMut() -> Result<(), HostProblem>,
    ) -> Result<(), HostProblem> {
        actual.require_current(actual.original())?;
        dispatch()
    }
    fn with_completed_plain_read(
        &mut self,
        actual: &DurableCompletedPlainRead<'_>,
        dispatch: &mut dyn FnMut(&(dyn std::any::Any + Send + Sync)) -> Result<(), HostProblem>,
    ) -> Result<(), HostProblem> {
        self.calls += 1;
        actual.require_current()?;
        assert_eq!(actual.effect().deadline_tick, 200);
        assert_eq!(actual.completed().intent.recovery_after_tick, 100);
        assert_eq!(actual.completed().state, EffectState::Completed);
        let mut retained = actual.completed().clone();
        if self.mode == "forged-original-bound" {
            // An offered clone is not the actual immutable core observation.
            retained.intent.recovery_after_tick = 200;
        }
        let prepared = self.cics.prepare_completed_plain_read(
            self.store.clone(),
            actual.original().clone(),
            actual.effect().clone(),
            retained,
            actual.execution().clone(),
            actual.control().now_tick,
        )?;
        dispatch(&prepared)
    }
}

fn mutation(sequence: u64) -> Mutation {
    Mutation {
        sequence,
        idempotency_key: IdempotencyKey::new(
            format!("read-deadline-seed:{sequence}"),
            InvocationLimits::default(),
        )
        .unwrap(),
        transaction: None,
    }
}

#[test]
fn completed_plain_read_preserves_original_occurrence_deadline_under_fresh_request_budget() {
    run_deadline_cases(&[
        "fresh-request",
        "original-bound-disagrees",
        "forged-original-bound",
        "current-expired",
        "cancelled",
        "current-SAF-denied",
        "alias-changed",
        "session-expired",
    ]);
}

#[test]
fn completed_plain_read_rechecks_live_clock_after_saf_before_atomic_replay() {
    run_deadline_cases(&[
        "post-SAF-clock-advance",
        "post-SAF-clock-regressed",
        "post-SAF-clock-below-store-floor",
        "post-SAF-clock-absent",
        "post-SAF-clock-zero",
        "post-SAF-clock-unavailable",
        "post-SAF-current-expired",
        "post-SAF-session-expired",
        "post-SAF-cancelled",
    ]);
}

fn run_deadline_cases(modes: &[&'static str]) {
    let artifact = crate::compile(
        "IDENTIFICATION DIVISION. PROGRAM-ID. READBND. \
         DATA DIVISION. WORKING-STORAGE SECTION. \
         01 REC PIC X(4). 01 REC-KEY PIC X(2) VALUE 'AA'. \
         PROCEDURE DIVISION. \
         EXEC CICS READ DATASET('ACCTDAT') INTO(REC) RIDFLD(REC-KEY) END-EXEC. \
         DISPLAY REC. STOP RUN.",
    )
    .unwrap();
    for &mode in modes {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let platform: Arc<dyn PlatformStore> = store.clone();
        let dataset = DatasetService::open(store.clone(), DatasetLimits::default()).unwrap();
        let name = DatasetName::new("TEST.ACCTDAT", 128).unwrap();
        dataset
            .invoke(DatasetRequest::Create {
                dataset: name.clone(),
                attributes: DatasetAttributes {
                    organization: DatasetOrganization::KeySequenced,
                    record_format: RecordFormat::Fixed,
                    logical_record_length: 4,
                    key_offset: Some(0),
                    key_length: Some(2),
                    ccsid: None,
                },
                mutation: mutation(1),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::Write {
                dataset: name.clone(),
                member: None,
                records: vec![b"AA11".to_vec()],
                expected_version: Some(1),
                mutation: mutation(2),
            })
            .unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        let deny = Arc::new(AtomicBool::new(false));
        let clock = Arc::new(Clock(AtomicU64::new(10)));
        let armed = Arc::new(AtomicBool::new(false));
        let cancellation = CancellationProbe::new();
        let mut ports: Vec<Arc<dyn HostProvider>> =
            dataset_providers(dataset, InvocationLimits::default())
                .into_iter()
                .map(|inner| {
                    Arc::new(DatasetReadCount {
                        inner,
                        reads: reads.clone(),
                    }) as Arc<dyn HostProvider>
                })
                .collect();
        ports.push(Arc::new(Security {
            descriptor: CapabilityDescriptor {
                capability: CapabilityId::new(
                    "host.security.authorize",
                    InvocationLimits::default(),
                )
                .unwrap(),
                provider_id: "read-deadline-security".into(),
                generation: "1".into(),
                request_schema: "request@1".into(),
                result_schema: "result@1".into(),
                max_request_bytes: 4096,
                max_result_bytes: 4096,
                ready: true,
            },
            deny: deny.clone(),
            store: store.clone(),
            clock: clock.clone(),
            armed: armed.clone(),
            mode,
            cancellation: cancellation.clone(),
        }));
        let nested_host = Arc::new(ScopedHostService::new(
            Arc::new(RegistrySnapshot::new(1, ports, InvocationLimits::default()).unwrap()),
            HostLimits::default(),
        ));
        let cics = CicsService::open_with_replay_clock(
            nested_host.clone(),
            store.clone(),
            CicsLimits::default(),
            clock.clone(),
        )
        .unwrap();
        cics.register_file_aliases(&BTreeMap::from([("ACCTDAT".into(), name)]))
            .unwrap();
        let host = Arc::new(ScopedHostService::new(
            Arc::new(
                RegistrySnapshot::new(
                    1,
                    vec![cics_provider(cics.clone(), InvocationLimits::default())],
                    InvocationLimits::default(),
                )
                .unwrap(),
            ),
            HostLimits::default(),
        ));
        let mut actor = crate::invocation(&artifact, 4096).with_cancellation_probe(cancellation);
        actor.selector = Selector::new("program:READBND", InvocationLimits::default()).unwrap();
        actor.deadline_tick = 100;
        actor.principal = Principal::new(
            actor.principal.id().clone(),
            [
                "host.cics.execute",
                "host.dataset.read",
                "host.security.authorize",
            ]
            .into_iter()
            .map(|name| CapabilityId::new(name, InvocationLimits::default()).unwrap())
            .collect::<BTreeSet<_>>(),
            InvocationLimits::default(),
        )
        .unwrap();
        let session = SessionId::new("read-deadline-session", 64).unwrap();
        let launched = cics
            .launch_terminal(
                actor.clone(),
                &session,
                "MENU",
                24,
                80,
                "fixture-csrf",
                1,
                match mode {
                    "session-expired" => 50,
                    "post-SAF-session-expired" => 170,
                    _ => 500,
                },
            )
            .unwrap();
        if mode == "post-SAF-session-expired" {
            // The public API takes an idle duration: launch1 + timeout170 = expiry171.
            assert_eq!(launched.expires_at_tick, 171);
        }
        if mode == "original-bound-disagrees" {
            // The public API permits a task longer than its actor/effect.
            // Its genuine writer max150 must not be guessed from core min100.
            let mut task = actor.clone();
            task.deadline_tick = 150;
            cics.restore_terminal_program_run(task, actor.clone(), &session, "MENU", vec![], 10)
                .unwrap();
        }
        let key = IdempotencyKey::new(
            format!("{}:1", actor.idempotency_key),
            InvocationLimits::default(),
        )
        .unwrap();
        let coordinator =
            ExecutionCoordinator::durable(host, store.clone(), CoordinatorLimits::default());
        let mut first_machine = ReferenceMachine::from_binary(
            artifact.payload(),
            actor.clone(),
            mainframe_env_ir::CodecLimits::default(),
        )
        .unwrap();
        let mut first_hook = Replay {
            cics: cics.clone(),
            store: platform.clone(),
            mode: "fresh-request",
            calls: 0,
        };
        let mut cut_seen = false;
        // Unwind only after the actual coordinator persisted Completed. No
        // fabricated EffectRecord or execution-state transition supplies proof.
        let cut = catch_unwind(AssertUnwindSafe(|| {
            coordinator.execute_resumable_with_dispatch_control(
                &mut first_machine,
                &actor,
                || {
                    if store
                        .effect(&key)
                        .unwrap()
                        .is_some_and(|e| e.state == EffectState::Completed)
                    {
                        cut_seen = true;
                        panic!("owned test cut after actual Completed READ");
                    }
                    Ok(ExecutionControl {
                        now_tick: 10,
                        cancellation_requested: false,
                    })
                },
                &mut first_hook,
            )
        }));
        assert!(cut.is_err() && cut_seen, "{mode}");
        let completed = store.effect(&key).unwrap().unwrap();
        assert_eq!(completed.state, EffectState::Completed);
        assert_eq!(completed.intent.recovery_after_tick, 100);
        assert_eq!(
            store
                .get_execution(&actor.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Running
        );
        let receipt = store
            .get_provider_state("cics-effect-replay-v1", key.as_str())
            .unwrap()
            .unwrap();
        let descriptor = mainframe_env_cics::describe_cics_replay_row(
            &receipt,
            Some(&completed),
            CicsLimits::default(),
        )
        .unwrap();
        assert_eq!(
            descriptor.deadline_tick,
            Some(if mode == "original-bound-disagrees" {
                150
            } else {
                100
            })
        );
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        clock.0.store(150, Ordering::SeqCst);
        // Reopen through the public clockless composition: retained V2 rows do not
        // authorize consumption when no fresh replay clock is configured.
        let cics = if mode == "post-SAF-clock-absent" {
            CicsService::open(nested_host, store.clone(), CicsLimits::default()).unwrap()
        } else {
            cics
        };
        let coordinator = if mode == "post-SAF-clock-absent" {
            let host = Arc::new(ScopedHostService::new(
                Arc::new(
                    RegistrySnapshot::new(
                        1,
                        vec![cics_provider(cics.clone(), InvocationLimits::default())],
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                ),
                HostLimits::default(),
            ));
            ExecutionCoordinator::durable(host, store.clone(), CoordinatorLimits::default())
        } else {
            coordinator
        };
        let mut current = actor.clone();
        current.deadline_tick = if mode == "current-expired" { 140 } else { 200 };
        if mode == "cancelled" {
            let cancellation = CancellationProbe::new();
            cancellation.request();
            current = current.with_cancellation_probe(cancellation);
        }
        if mode == "current-SAF-denied" {
            deny.store(true, Ordering::SeqCst);
        }
        if mode == "alias-changed" {
            let alias = store
                .get_provider_state("cics-file-alias", "ACCTDAT")
                .unwrap()
                .unwrap();
            store
                .put_provider_state(
                    ProviderStateRecord {
                        version: alias.version + 1,
                        ..alias.clone()
                    },
                    Some(alias.version),
                )
                .unwrap();
        }
        let restored = cics.restore_terminal_program_run(
            current.clone(),
            current.clone(),
            &session,
            "MENU",
            vec![],
            150,
        );
        if mode == "session-expired" {
            assert!(restored.is_err());
        } else {
            restored.unwrap();
            armed.store(mode.starts_with("post-SAF-"), Ordering::SeqCst);
            let mut machine = ReferenceMachine::from_binary(
                artifact.payload(),
                current.clone(),
                mainframe_env_ir::CodecLimits::default(),
            )
            .unwrap();
            let mut hook = Replay {
                cics,
                store: platform,
                mode,
                calls: 0,
            };
            let outcome = coordinator.execute_resumable_with_dispatch_control(
                &mut machine,
                &current,
                || {
                    Ok(ExecutionControl {
                        now_tick: 150,
                        cancellation_requested: false,
                    })
                },
                &mut hook,
            );
            match mode {
                "fresh-request" | "post-SAF-clock-advance" => {
                    let ExecutionOutcome::Completed(done) = outcome else {
                        panic!("{outcome:?}")
                    };
                    assert_eq!(done.output.bytes(), b"AA11\n");
                    assert_eq!(hook.calls, 1);
                    if mode == "post-SAF-clock-advance" {
                        assert_eq!(store.advance_logical_clock(0).unwrap(), 160);
                    }
                    assert_eq!(
                        store
                            .events(&actor.execution_id, 0, 32)
                            .unwrap()
                            .iter()
                            .filter(|event| matches!(
                                event.kind,
                                LifecycleEventKind::EffectResult { sequence: 1 }
                            ))
                            .count(),
                        2
                    );
                }
                "current-expired" => assert!(matches!(outcome, ExecutionOutcome::TimedOut)),
                "cancelled" => assert!(matches!(outcome, ExecutionOutcome::Cancelled)),
                _ => assert!(
                    matches!(outcome, ExecutionOutcome::ProviderFailure(problem) if problem.has_unknown_outcome()),
                    "{mode}"
                ),
            }
        }
        assert_eq!(store.effect(&key).unwrap(), Some(completed), "{mode}");
        assert_eq!(
            store
                .get_provider_state("cics-effect-replay-v1", key.as_str())
                .unwrap(),
            Some(receipt),
            "{mode}"
        );
        assert_eq!(reads.load(Ordering::SeqCst), 1, "{mode}");
    }
}
