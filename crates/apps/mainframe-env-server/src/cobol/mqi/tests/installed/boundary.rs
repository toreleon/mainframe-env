//! Real compiled/published installed child and coordinator/core/CALL admission.
//! The parent driver/frame/MQ provider are fixtures, not selected service/SAF.
use super::*;
use mainframe_env_host_api::ProgramName;
use mainframe_env_store_api::{EffectDigestFormat, ExecutionState, ProviderStateStore};
use std::sync::atomic::{AtomicBool, AtomicU64};
mod lookup;

const SOURCE: &str = "IDENTIFICATION DIVISION. PROGRAM-ID. MQFLOW. DATA DIVISION. WORKING-STORAGE SECTION. 01 QM PIC X(48) VALUE SPACES. 01 HC PIC S9(9) BINARY. 01 CC PIC S9(9) BINARY. 01 RC PIC S9(9) BINARY. PROCEDURE DIVISION. CALL 'MQCONN' USING QM HC CC RC. CALL 'MQDISC' USING HC CC RC. DISPLAY 'DONE'. STOP RUN.";
type Admit = dyn Fn(&InstalledBatchAdmission<'_>) -> Result<Box<dyn InstalledMqFrameSession>, HostProblem>
    + Send
    + Sync;
struct Factory(Box<Admit>);
impl ProgramMqHostAdmission for Factory {
    fn admit_installed_batch(
        &self,
        proof: &InstalledBatchAdmission<'_>,
    ) -> Result<Box<dyn InstalledMqFrameSession>, HostProblem> {
        (self.0)(proof)
    }
}
type Hook = dyn Fn(&mut Invocation, &mut EffectRequest) + Send + Sync;
struct RouterHook {
    router: WeakProgramRouter,
    hook: Box<Hook>,
}
impl HostProvider for RouterHook {
    fn descriptor(&self) -> &CapabilityDescriptor {
        self.router.descriptor()
    }
    fn invoke(&self, invocation: &Invocation, mut effect: EffectRequest) -> EffectResult {
        let mut invocation = invocation.clone();
        (self.hook)(&mut invocation, &mut effect);
        self.router.invoke(&invocation, effect)
    }
}
struct Fixture {
    _root: super::super::super::super::hardening::TestRoot,
    store: Arc<dyn PlatformStore>,
    router: Arc<DefaultProgramRouter>,
    host: Arc<ScopedHostService>,
    provider: Arc<Provider>,
    tick: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
    url: String,
    sqlite: bool,
}
impl Fixture {
    fn new(sqlite: bool) -> Self {
        let root = super::super::super::super::hardening::TestRoot::new();
        let url = format!("sqlite://{}?mode=rwc", root.0.join("boundary.db").display());
        let store: Arc<dyn PlatformStore> = if sqlite {
            Arc::new(SqliteStateStore::open(&url, 8 * 1024 * 1024, 65536).unwrap())
        } else {
            Arc::new(MemoryStore::new(Default::default()))
        };
        let router = default_program_router();
        let tick = Arc::new(AtomicU64::new(1));
        let cancel = Arc::new(AtomicBool::new(false));
        let observed_tick = tick.clone();
        let observed_cancel = cancel.clone();
        router
            .bind_execution_control(Arc::new(move |_: &Invocation| {
                Ok(ExecutionControl {
                    now_tick: observed_tick.load(Ordering::SeqCst),
                    cancellation_requested: observed_cancel.load(Ordering::SeqCst),
                })
            }))
            .unwrap();
        let provider = Arc::new(Provider {
            descriptor: CapabilityDescriptor {
                capability: CapabilityId::new("host.mq.write", InvocationLimits::default())
                    .unwrap(),
                provider_id: "fixture-mq".into(),
                generation: "fixture@1".into(),
                request_schema: "mainframe-env.host-request@1".into(),
                result_schema: "mainframe-env.host-result@1".into(),
                max_request_bytes: 8 * 1024 * 1024,
                max_result_bytes: 8 * 1024 * 1024,
                ready: true,
            },
            store: store.clone(),
            registry: Mutex::new(MqHandleRegistry::new(1, 4).unwrap()),
            keys: Mutex::new(vec![]),
            failure: Mutex::new(None),
            cancel_after_connect: AtomicBool::new(false),
        });
        let host = Arc::new(ScopedHostService::new(
            Arc::new(
                RegistrySnapshot::new(
                    1,
                    vec![
                        Arc::new(WeakProgramRouter::new(&router)) as Arc<dyn HostProvider>,
                        provider.clone(),
                    ],
                    InvocationLimits::default(),
                )
                .unwrap(),
            ),
            Default::default(),
        ));
        Self {
            _root: root,
            store,
            router,
            host,
            provider,
            tick,
            cancel,
            url,
            sqlite,
        }
    }
    fn bind(&mut self, factory: Arc<dyn ProgramMqHostAdmission>, hook: Option<Box<Hook>>) {
        self.bind_source(factory, hook, SOURCE);
    }
    fn bind_source(
        &mut self,
        factory: Arc<dyn ProgramMqHostAdmission>,
        hook: Option<Box<Hook>>,
        source: &str,
    ) {
        self.bind_optional(Some(factory), hook, source);
    }
    fn bind_optional(
        &mut self,
        factory: Option<Arc<dyn ProgramMqHostAdmission>>,
        hook: Option<Box<Hook>>,
        source: &str,
    ) {
        if let Some(factory) = factory {
            self.router.bind_mqi_program_host(factory).unwrap();
        }
        if let Some(hook) = hook {
            let wrapper: Arc<dyn HostProvider> = Arc::new(RouterHook {
                router: WeakProgramRouter::new(&self.router),
                hook,
            });
            self.host = Arc::new(ScopedHostService::new(
                Arc::new(
                    RegistrySnapshot::new(
                        1,
                        vec![wrapper, self.provider.clone()],
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                ),
                Default::default(),
            ));
        }
        let artifacts =
            Arc::new(LocalArtifactStore::open(&self._root.0, 64 * 1024 * 1024).unwrap());
        self.router
            .bind_runtime(self.host.clone(), self.store.clone(), artifacts.clone())
            .unwrap();
        for (name, source) in [
            ("MQFLOW", source),
            (
                "CALLER",
                "IDENTIFICATION DIVISION. PROGRAM-ID. CALLER. PROCEDURE DIVISION. STOP RUN.",
            ),
        ] {
            let input = super::super::super::super::hardening::input(source);
            let CompilerResult::Published { artifact, .. } = CobolCompiler::default()
                .compile(CompilerRequest {
                    source: source_bundle(&input).unwrap(),
                    mode: CompilationMode::Executable,
                    target: CompileTarget::new("reference").unwrap(),
                    options: CompileOptions::new(BTreeMap::new()).unwrap(),
                })
                .unwrap()
            else {
                panic!("compiler")
            };
            let record = artifact::published_artifact_record(&artifact).unwrap();
            let id = record.artifact.clone();
            artifacts.put_artifact(record).unwrap();
            self.store
                .put_provider_state(
                    ProviderStateRecord {
                        namespace: "batch-program".into(),
                        key: name.into(),
                        version: 1,
                        payload: id.as_str().as_bytes().to_vec(),
                    },
                    None,
                )
                .unwrap();
        }
    }
    fn parent(&self) -> Invocation {
        let mut parent = super::super::super::super::hardening::parent();
        parent.deadline_tick = 1000;
        parent.cancellation_probe = Some(mainframe_env_execution_api::CancellationProbe::new());
        parent.selector = Selector::new("program:CALLER", InvocationLimits::default()).unwrap();
        let record = self
            .store
            .get_provider_state("batch-program", "CALLER")
            .unwrap()
            .unwrap();
        parent.artifact = ArtifactRef::new(
            String::from_utf8(record.payload).unwrap(),
            InvocationLimits::default(),
        )
        .unwrap();
        let mut grants = parent.principal.grants().clone();
        grants.insert(CapabilityId::new("host.mq.write", InvocationLimits::default()).unwrap());
        parent.principal = Principal::new(
            parent.principal.id().clone(),
            grants,
            InvocationLimits::default(),
        )
        .unwrap();
        parent
    }
    fn effect(&self, parent: &Invocation, input: ProgramInput) -> EffectRequest {
        EffectRequest {
            run_unit: parent.run_unit_id.clone(),
            sequence: 1,
            deadline_tick: parent.deadline_tick,
            idempotency_key: Some(
                IdempotencyKey::new("real-enclosing-call", InvocationLimits::default()).unwrap(),
            ),
            request: HostRequest::Program(ProgramRequest::Call {
                program: ProgramName::new("MQFLOW", 128).unwrap(),
                payload: BoundedPayload::new(
                    "mainframe-env.program.input@1",
                    serde_json::to_vec(&input).unwrap(),
                    InvocationLimits::default(),
                )
                .unwrap(),
                service: None,
            }),
        }
    }
    fn run(&self, input: ProgramInput) -> (ExecutionOutcome, Option<EffectResult>) {
        let parent = self.parent();
        self.run_parent(parent, input)
    }
    fn run_parent(
        &self,
        parent: Invocation,
        input: ProgramInput,
    ) -> (ExecutionOutcome, Option<EffectResult>) {
        let effect = self.effect(&parent, input);
        let mut driver = OriginalCaller {
            original: Some(effect),
            reply: None,
        };
        let outcome = ExecutionCoordinator::durable(
            self.host.clone(),
            self.store.clone(),
            Default::default(),
        )
        .execute_with_control(&mut driver, &parent, || {
            Ok(ExecutionControl {
                now_tick: 1,
                cancellation_requested: false,
            })
        });
        (outcome, driver.reply)
    }
    fn pending(&self) -> Vec<ProviderStateRecord> {
        self.store
            .list_provider_state(
                super::super::super::super::retention::CALL_REPLAY_NAMESPACE,
                8,
            )
            .unwrap()
    }
    fn assert_no_mq(&self) {
        assert!(self.provider.keys.lock().unwrap().is_empty());
    }
    fn close_reopen(self) {
        let pending = self.pending();
        let Fixture {
            _root,
            store,
            router,
            host,
            provider,
            tick: _,
            cancel: _,
            url,
            sqlite,
        } = self;
        drop(router);
        drop(host);
        drop(provider);
        drop(store);
        if sqlite {
            let reopened = SqliteStateStore::open(&url, 8 * 1024 * 1024, 65536).unwrap();
            assert_eq!(
                reopened
                    .list_provider_state(
                        super::super::super::super::retention::CALL_REPLAY_NAMESPACE,
                        8
                    )
                    .unwrap(),
                pending
            );
        }
    }
}
fn input() -> ProgramInput {
    super::super::super::super::hardening::input("")
}
fn session(
    proof: &InstalledBatchAdmission<'_>,
    observed: &Arc<Admission>,
) -> Box<dyn InstalledMqFrameSession> {
    Box::new(Session {
        frame: Arc::new(Frame(proof.child().clone())),
        events: observed.events.clone(),
        aborts: observed.aborts.clone(),
        store: proof.store().clone(),
        control: proof.execution_control().clone(),
    })
}

#[test]
fn genuine_admission_observes_full_provenance_and_reentry_has_no_second_dispatch() {
    for sqlite in [false, true] {
        let mut fixture = Fixture::new(sqlite);
        let observed = admission(false);
        let recorded = observed.clone();
        let router = Arc::downgrade(&fixture.router);
        let expected = fixture.store.clone();
        fixture.bind(
            Arc::new(Factory(Box::new(move |proof| {
                let router = router.upgrade().unwrap();
                assert!(router.cobol.setup.try_lock().is_ok());
                assert_eq!(
                    router.bind_execution_control(super::super::setup::control()),
                    Err(HostProblem::IdempotencyConflict)
                );
                assert!(Arc::ptr_eq(proof.store(), &expected));
                assert!(Arc::ptr_eq(
                    proof.execution_control(),
                    router.cobol.control.get().unwrap()
                ));
                assert_eq!(
                    proof.child().parent_execution_id.as_ref(),
                    Some(&proof.parent().execution_id)
                );
                assert_eq!(proof.child().principal, proof.parent().principal);
                assert_eq!(proof.child().deadline_tick, proof.parent().deadline_tick);
                assert_eq!(
                    proof.child().provider_generations,
                    proof.parent().provider_generations
                );
                assert_eq!(
                    proof.child().cancellation_probe,
                    proof.parent().cancellation_probe
                );
                assert_eq!(proof.running_parent().state, ExecutionState::Running);
                assert_eq!(
                    proof.core_intent().digest_format,
                    EffectDigestFormat::CanonicalHostV1
                );
                assert_eq!(
                    proof.core_intent().request_digest,
                    canonical_request_digest(&proof.original_call().request).unwrap()
                );
                assert_eq!(proof.call_reservation().version, 1);
                assert!(
                    proof
                        .manifest()
                        .host_interfaces
                        .contains("mainframe-env.host@1")
                );
                assert!(
                    proof
                        .artifact_metadata()
                        .validates_payload(&proof.content_digest())
                );
                assert!(proof.selection().is_none());
                let duplicate = router.invoke(proof.parent(), proof.original_call().clone());
                assert_eq!(duplicate.outcome, Err(HostProblem::UnknownOutcome));
                Ok(session(proof, &recorded))
            }))),
            None,
        );
        let (outcome, reply) = fixture.run(input());
        assert!(
            matches!(outcome, ExecutionOutcome::Completed(_)),
            "{outcome:?}"
        );
        assert!(reply.unwrap().outcome.is_ok());
        assert_eq!(fixture.provider.keys.lock().unwrap().len(), 2);
        assert_eq!(observed.events.lock().unwrap().len(), 1);
        assert!(observed.aborts.lock().unwrap().is_empty());
        assert_eq!(fixture.pending()[0].version, 2);
        fixture.close_reopen();
    }
}

#[test]
fn absent_stale_foreign_or_changed_parent_occurrence_cannot_reserve_or_admit() {
    for sqlite in [false, true] {
        for mode in 0..6 {
            let mut fixture = Fixture::new(sqlite);
            let observed = admission(false);
            fixture.bind(
                observed.clone(),
                Some(Box::new(move |parent, effect| match mode {
                    0 => {
                        parent.execution_id =
                            ExecutionId::new("foreign", InvocationLimits::default()).unwrap()
                    }
                    1 => parent.attempt += 1,
                    2 => effect.sequence += 1,
                    3 => effect.deadline_tick -= 1,
                    4 => {
                        effect.idempotency_key = Some(
                            IdempotencyKey::new("missing", InvocationLimits::default()).unwrap(),
                        )
                    }
                    _ => {
                        let HostRequest::Program(ProgramRequest::Call { payload, .. }) =
                            &mut effect.request
                        else {
                            panic!()
                        };
                        *payload = BoundedPayload::new(
                            "mainframe-env.program.input@1",
                            b"{}".to_vec(),
                            InvocationLimits::default(),
                        )
                        .unwrap();
                    }
                })),
            );
            let (_, reply) = fixture.run(input());
            assert_eq!(
                reply.unwrap().outcome,
                Err(if mode == 2 {
                    HostProblem::Malformed
                } else {
                    HostProblem::Unauthorized
                }),
                "mode={mode}"
            );
            assert!(observed.observed.lock().unwrap().is_empty());
            fixture.assert_no_mq();
            assert!(fixture.pending().is_empty());
            fixture.close_reopen();
        }
    }
}

#[test]
fn post_admission_cancellation_deadline_regression_and_catalog_change_abort_once_keep_pending() {
    for sqlite in [false, true] {
        for mode in 0..4 {
            let mut fixture = Fixture::new(sqlite);
            let observed = admission(false);
            if mode == 2 {
                fixture.tick.store(2, Ordering::SeqCst);
            }
            let cancel = fixture.cancel.clone();
            let tick = fixture.tick.clone();
            let recorded = observed.clone();
            fixture.bind(
                Arc::new(Factory(Box::new(move |proof| {
                    match mode {
                        0 => cancel.store(true, Ordering::SeqCst),
                        1 => tick.store(proof.parent().deadline_tick, Ordering::SeqCst),
                        2 => tick.store(1, Ordering::SeqCst),
                        _ => {
                            let mut catalog = proof.catalog_record().unwrap().clone();
                            let old = catalog.version;
                            catalog.version += 1;
                            proof
                                .store()
                                .put_provider_state(catalog, Some(old))
                                .unwrap();
                        }
                    }
                    Ok(session(proof, &recorded))
                }))),
                None,
            );
            let (_, reply) = fixture.run(input());
            let expected = match mode {
                0 => HostProblem::Cancelled,
                1 => HostProblem::TimedOut,
                2 => HostProblem::InfrastructureFailure,
                _ => HostProblem::Unauthorized,
            };
            assert_eq!(reply.unwrap().outcome, Err(expected.clone()));
            assert_eq!(observed.aborts.lock().unwrap().as_slice(), &[expected]);
            assert!(observed.events.lock().unwrap().is_empty());
            fixture.assert_no_mq();
            let pending = fixture.pending();
            assert_eq!(pending.len(), 1);
            assert_eq!(pending[0].version, 1);
            let state: serde_json::Value = serde_json::from_slice(&pending[0].payload).unwrap();
            assert!(state["reply"].is_null());
            fixture.close_reopen();
        }
    }
}

struct BrokenSession {
    inner: Box<dyn InstalledMqFrameSession>,
    mode: u8,
}
impl InstalledMqFrameSession for BrokenSession {
    fn store(&self) -> &Arc<dyn PlatformStore> {
        self.inner.store()
    }
    fn execution_control(&self) -> &Arc<dyn ProgramExecutionControl> {
        self.inner.execution_control()
    }
    fn program_frame(&self) -> Result<Arc<dyn MqMqiProgramFrame>, HostProblem> {
        match self.mode {
            0 => Err(HostProblem::ProviderFailure),
            1 => panic!("preparation callback panic"),
            3 => {
                let mut invocation = super::super::super::super::hardening::parent();
                invocation.artifact =
                    ArtifactRef::new("wrong-frame", InvocationLimits::default()).unwrap();
                Ok(Arc::new(Frame(invocation)))
            }
            _ => self.inner.program_frame(),
        }
    }
    fn abort_preparation(&mut self, problem: &HostProblem) -> Result<(), HostProblem> {
        self.inner.abort_preparation(problem)
    }
    fn finish(&mut self, raw: &ExecutionOutcome) -> Result<(), HostProblem> {
        self.inner.finish(raw)?;
        if self.mode == 2 {
            Err(HostProblem::ProviderFailure)
        } else {
            Ok(())
        }
    }
}

#[test]
fn physical_store_identity_is_not_replaced_by_equal_core_catalog_and_call_rows() {
    {
        let mut fixture = Fixture::new(true);
        let foreign_url = format!(
            "sqlite://{}?mode=rwc",
            fixture._root.0.join("foreign.db").display()
        );
        let service_store: Arc<dyn PlatformStore> =
            Arc::new(SqliteStateStore::open(&foreign_url, 8 * 1024 * 1024, 65536).unwrap());
        fixture.bind(
            Arc::new(Factory(Box::new(move |proof| {
                // Negative replica fixture only: the source's parent/core rows
                // were produced by the real coordinator, not seeded admission.
                for record in proof
                    .store()
                    .list_provider_state_prefix("durable-", 64)
                    .unwrap()
                {
                    assert!(record.version <= 64);
                    for version in 1..=record.version {
                        let mut copied = record.clone();
                        copied.version = version;
                        service_store
                            .put_provider_state(
                                copied,
                                if version == 1 {
                                    None
                                } else {
                                    Some(version - 1)
                                },
                            )
                            .unwrap();
                    }
                }
                service_store
                    .put_provider_state(proof.catalog_record().unwrap().clone(), None)
                    .unwrap();
                service_store
                    .put_provider_state(proof.call_reservation().clone(), None)
                    .unwrap();
                assert_eq!(
                    service_store
                        .get_execution(&proof.parent().execution_id)
                        .unwrap()
                        .as_ref(),
                    Some(proof.running_parent())
                );
                assert_eq!(
                    service_store
                        .effect(proof.original_call().idempotency_key.as_ref().unwrap())
                        .unwrap()
                        .as_ref(),
                    Some(proof.core_intent())
                );
                assert!(!Arc::ptr_eq(proof.store(), &service_store));
                // Required bridge check: observations from equal foreign rows do NOT
                // identify the service's bound physical adapter or admit its frame.
                // Return a coincident frame on that foreign physical adapter.
                // The SERVER must refuse it and abort preparation, even though
                // the factory returned Ok and every copied observation matches.
                Ok(Box::new(Session {
                    frame: Arc::new(Frame(proof.child().clone())),
                    events: Arc::new(Mutex::new(vec![])),
                    aborts: Arc::new(Mutex::new(vec![])),
                    store: service_store.clone(),
                    control: proof.execution_control().clone(),
                }))
            }))),
            None,
        );
        let (_, reply) = fixture.run(input());
        assert_eq!(reply.unwrap().outcome, Err(HostProblem::Unauthorized));
        fixture.assert_no_mq();
        assert_eq!(fixture.pending()[0].version, 1);
        fixture.close_reopen();
    }
}

#[test]
fn raw_unknown_from_real_child_core_is_finished_once_and_remains_protected_on_retry() {
    for sqlite in [false, true] {
        let mut fixture = Fixture::new(sqlite);
        let observed = admission(false);
        fixture.bind(observed.clone(), None);
        *fixture.provider.failure.lock().unwrap() = Some(HostProblem::UnknownOutcome);
        let (outcome, reply) = fixture.run(input());
        assert!(
            matches!(outcome, ExecutionOutcome::ProviderFailure(ref problem) if problem.has_unknown_outcome()),
            "{outcome:?}"
        );
        assert!(reply.is_none());
        let events = observed.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ExecutionOutcome::ProviderFailure(problem) if problem.has_unknown_outcome())
        );
        drop(events);
        assert!(observed.aborts.lock().unwrap().is_empty());
        let pending = fixture.pending();
        assert_eq!(pending[0].version, 1);
        let parent = fixture.parent();
        let original = fixture.effect(&parent, input());
        let core = fixture
            .store
            .effect(original.idempotency_key.as_ref().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(core.state, EffectState::UnknownOutcome);
        let retry = fixture.router.invoke(&parent, original.clone());
        assert_eq!(retry.outcome, Err(HostProblem::UnknownOutcome));
        assert_eq!(fixture.pending(), pending);
        assert_eq!(
            fixture
                .store
                .effect(original.idempotency_key.as_ref().unwrap())
                .unwrap(),
            Some(core)
        );
        assert_eq!(fixture.provider.keys.lock().unwrap().len(), 1);
        assert_eq!(observed.observed.lock().unwrap().len(), 1);
        fixture.close_reopen();
    }
}

#[test]
fn frame_bind_and_environment_setup_failures_abort_only_volatile_admission() {
    for sqlite in [false, true] {
        for wrong_frame in [false, true] {
            let mut fixture = Fixture::new(sqlite);
            let observed = admission(false);
            let recorded = observed.clone();
            fixture.bind_source(
                Arc::new(Factory(Box::new(move |proof| {
                    Ok(Box::new(BrokenSession {
                        inner: session(proof, &recorded),
                        mode: if wrong_frame { 3 } else { 4 },
                    }))
                }))),
                None,
                &if wrong_frame {
                    SOURCE.to_string()
                } else {
                    SOURCE.replace("01 QM", "01 PSAPTR USAGE POINTER. 01 QM")
                },
            );
            let mut input = input();
            if !wrong_frame {
                input.execution = Some(mainframe_env_batch::ProgramExecutionContext {
                    job_name: "bad name".into(),
                    step_name: "STEP".into(),
                });
            }
            let (_, reply) = fixture.run(input);
            let expected = if wrong_frame {
                HostProblem::Unauthorized
            } else {
                HostProblem::Malformed
            };
            assert_eq!(reply.unwrap().outcome, Err(expected.clone()));
            assert_eq!(observed.aborts.lock().unwrap().as_slice(), &[expected]);
            assert!(observed.events.lock().unwrap().is_empty());
            fixture.assert_no_mq();
            assert_eq!(fixture.pending()[0].version, 1);
            fixture.close_reopen();
        }
    }
}

#[test]
fn setup_or_finish_failure_preserves_reservation_and_unknown_precedence_on_real_route() {
    for sqlite in [false, true] {
        for mode in 0..3 {
            let mut fixture = Fixture::new(sqlite);
            let observed = admission(false);
            let recorded = observed.clone();
            fixture.bind(
                Arc::new(Factory(Box::new(move |proof| {
                    Ok(Box::new(BrokenSession {
                        inner: session(proof, &recorded),
                        mode,
                    }))
                }))),
                None,
            );
            let (outcome, reply) = fixture.run(input());
            if mode == 0 {
                assert_eq!(reply.unwrap().outcome, Err(HostProblem::ProviderFailure));
                fixture.assert_no_mq();
                assert_eq!(
                    observed.aborts.lock().unwrap().as_slice(),
                    &[HostProblem::ProviderFailure]
                );
            } else {
                assert!(
                    matches!(outcome, ExecutionOutcome::ProviderFailure(ref problem) if problem.has_unknown_outcome()),
                    "{outcome:?}"
                );
                assert!(reply.is_none());
                if mode == 1 {
                    fixture.assert_no_mq();
                    assert_eq!(
                        observed.aborts.lock().unwrap().as_slice(),
                        &[HostProblem::UnknownOutcome]
                    );
                } else {
                    assert_eq!(observed.events.lock().unwrap().len(), 1);
                    assert_eq!(fixture.provider.keys.lock().unwrap().len(), 2);
                }
            }
            assert_eq!(fixture.pending()[0].version, 1);
            fixture.close_reopen();
        }
    }
}

#[test]
fn direct_typed_helper_without_original_proof_fails_closed_while_legacy_remains_usable() {
    let mut fixture = Fixture::new(false);
    fixture.bind(admission(false), None);
    let parent = fixture.parent();
    let admitted = fixture
        .router
        .cobol
        .preflight_installed_program("MQFLOW", true)
        .unwrap();
    let payload = BoundedPayload::new(
        "mainframe-env.program.input@1",
        serde_json::to_vec(&input()).unwrap(),
        InvocationLimits::default(),
    )
    .unwrap();
    assert!(matches!(
        fixture.router.cobol.execute_installed_batch(
            &parent,
            "MQFLOW",
            admitted,
            &payload,
            "no-original"
        ),
        Err(HostProblem::Unsupported)
    ));
    fixture.assert_no_mq();
    assert!(fixture.pending().is_empty());

    let mut legacy = Fixture::new(false);
    legacy.bind_optional(None, None, "IDENTIFICATION DIVISION. PROGRAM-ID. MQFLOW. PROCEDURE DIVISION. DISPLAY 'LEGACY'. STOP RUN.");
    let admitted = legacy
        .router
        .cobol
        .preflight_installed_program("MQFLOW", true)
        .unwrap();
    let output = legacy
        .router
        .cobol
        .execute_installed_batch(
            &legacy.parent(),
            "MQFLOW",
            admitted,
            &payload,
            "legacy-direct",
        )
        .unwrap();
    assert_eq!(output.records, vec![b"LEGACY".to_vec()]);
    assert!(legacy.pending().is_empty());
}

#[test]
fn validated_binary_exceeding_actual_inherited_storage_aborts_before_child_dispatch() {
    for sqlite in [false, true] {
        let mut fixture = Fixture::new(sqlite);
        let observed = admission(false);
        fixture.bind(observed.clone(), None);
        let mut parent = fixture.parent();
        parent.limits.max_storage_bytes = 1;
        let (_, reply) = fixture.run_parent(parent, input());
        assert_eq!(reply.unwrap().outcome, Err(HostProblem::ProviderFailure));
        assert_eq!(
            observed.aborts.lock().unwrap().as_slice(),
            &[HostProblem::ProviderFailure]
        );
        assert!(observed.events.lock().unwrap().is_empty());
        fixture.assert_no_mq();
        assert_eq!(fixture.pending()[0].version, 1);
        fixture.close_reopen();
    }
}

#[test]
fn live_shared_probe_cancellation_reaches_raw_finish_without_inventing_disconnect() {
    for sqlite in [false, true] {
        let mut fixture = Fixture::new(sqlite);
        let observed = admission(false);
        fixture.bind(observed.clone(), None);
        fixture
            .provider
            .cancel_after_connect
            .store(true, Ordering::SeqCst);
        let (outcome, reply) = fixture.run(input());
        assert_eq!(outcome, ExecutionOutcome::Cancelled);
        assert!(reply.is_none());
        assert_eq!(
            observed.events.lock().unwrap().as_slice(),
            &[ExecutionOutcome::Cancelled]
        );
        assert!(observed.aborts.lock().unwrap().is_empty());
        assert_eq!(fixture.provider.keys.lock().unwrap().len(), 1);
        // A child's cancellation/return is NOT a proof that the parent's MQ task
        // ended. This session transport must not synthesize MQDISC/backout.
        assert_eq!(
            fixture.provider.registry.lock().unwrap().active_handles(),
            1
        );
        assert_eq!(fixture.pending()[0].version, 1);
        fixture.close_reopen();
    }
}

#[test]
fn coincident_control_values_cannot_replace_the_frozen_physical_control() {
    for sqlite in [false, true] {
        let mut fixture = Fixture::new(sqlite);
        let observed = admission(false);
        let recorded = observed.clone();
        fixture.bind(
            Arc::new(Factory(Box::new(move |proof| {
                let foreign = super::super::setup::control();
                assert_eq!(
                    foreign.observe(proof.parent()).unwrap(),
                    proof.execution_control().observe(proof.parent()).unwrap()
                );
                assert!(!Arc::ptr_eq(&foreign, proof.execution_control()));
                Ok(Box::new(Session {
                    frame: Arc::new(Frame(proof.child().clone())),
                    store: proof.store().clone(),
                    control: foreign,
                    events: recorded.events.clone(),
                    aborts: recorded.aborts.clone(),
                }))
            }))),
            None,
        );
        let (_, reply) = fixture.run(input());
        assert_eq!(reply.unwrap().outcome, Err(HostProblem::Unauthorized));
        assert_eq!(
            observed.aborts.lock().unwrap().as_slice(),
            &[HostProblem::Unauthorized]
        );
        assert!(observed.events.lock().unwrap().is_empty());
        fixture.assert_no_mq();
        assert_eq!(fixture.pending()[0].version, 1);
        fixture.close_reopen();
    }
}

#[test]
fn running_parent_or_call_reservation_retired_during_preparation_aborts_no_dispatch() {
    for sqlite in [false, true] {
        for retire_parent in [false, true] {
            let mut fixture = Fixture::new(sqlite);
            let observed = admission(false);
            let recorded = observed.clone();
            fixture.bind(
                Arc::new(Factory(Box::new(move |proof| {
                    if retire_parent {
                        proof
                            .store()
                            .transition_execution(
                                &proof.parent().execution_id,
                                proof.running_parent().version,
                                ExecutionState::Failed,
                                1,
                            )
                            .unwrap();
                    } else {
                        let mut record = proof.call_reservation().clone();
                        record.version += 1;
                        proof.store().put_provider_state(record, Some(1)).unwrap();
                    }
                    Ok(session(proof, &recorded))
                }))),
                None,
            );
            let _ = fixture.run(input());
            assert_eq!(observed.aborts.lock().unwrap().len(), 1);
            assert!(observed.events.lock().unwrap().is_empty());
            fixture.assert_no_mq();
            assert_eq!(
                fixture.pending()[0].version,
                if retire_parent { 1 } else { 2 }
            );
            fixture.close_reopen();
        }
    }
}
