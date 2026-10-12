use super::*;
use mainframe_env_cics::{CicsJavaStatus, CicsLimits, CicsProgramDefinition};
use mainframe_env_execution_api::{CapabilityId, PrincipalId};
use mainframe_env_host_api::{HostLimits, RegistrySnapshot, SecurityDecision, SessionId};
use mainframe_env_store::MemoryStore;
use mainframe_env_store_api::{
    ArtifactStore, EffectState, ExecutionState, ExecutionStore, IdempotencyStore,
    ProviderStateStore, ROOT_DRIVER_NAMESPACE,
};
use std::sync::atomic::AtomicUsize;

const SOURCE: &str = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSENTR. PROCEDURE DIVISION. CALL 'PROBE'. DISPLAY 'SECOND-DRIVE-FORBIDDEN'. GOBACK.";
struct Probe {
    descriptor: CapabilityDescriptor,
    calls: Arc<AtomicUsize>,
}
impl HostProvider for Probe {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }
    fn invoke(&self, original: &Invocation, effect: EffectRequest) -> EffectResult {
        let outcome = match &effect.request {
            HostRequest::Security(_) => Ok(HostResult::Security(SecurityDecision::Allow)),
            HostRequest::Program(ProgramRequest::Call {
                program,
                payload,
                service: None,
            }) if program.as_str() == "PROBE"
                && payload.schema() == "mainframe-env.cobol.call@1" =>
            {
                assert_eq!(original.selector.as_str(), "program:CICSENTR");
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(HostResult::Program(encode_cobol_call_result(&[]).unwrap()))
            }
            _ => Err(HostProblem::Unsupported),
        };
        EffectResult {
            sequence: effect.sequence,
            outcome,
        }
    }
}
fn descriptor(capability: &str) -> CapabilityDescriptor {
    CapabilityDescriptor {
        capability: CapabilityId::new(capability, Default::default()).unwrap(),
        provider_id: "compiled-cics-infrastructure-probe".into(),
        generation: "probe@1".into(),
        request_schema: "probe-request@1".into(),
        result_schema: "probe-result@1".into(),
        max_request_bytes: 1024 * 1024,
        max_result_bytes: 1024 * 1024,
        ready: true,
    }
}
struct Fixture {
    router: Arc<DefaultProgramRouter>,
    cics: Arc<CicsService>,
    store: Arc<MemoryStore>,
    terminal: Invocation,
    actor: Invocation,
    calls: Arc<AtomicUsize>,
}
fn fixture(foreign_adapters: bool) -> Fixture {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let calls = Arc::new(AtomicUsize::new(0));
    let scoped = Arc::new(ScopedHostService::new(
        Arc::new(
            RegistrySnapshot::new(
                1,
                ["host.security.authorize", "host.program.invoke"]
                    .into_iter()
                    .map(|name| {
                        Arc::new(Probe {
                            descriptor: descriptor(name),
                            calls: calls.clone(),
                        }) as Arc<dyn HostProvider>
                    })
                    .collect(),
                Default::default(),
            )
            .unwrap(),
        ),
        HostLimits::default(),
    ));
    let router = default_program_router();
    router
        .bind_execution_control(Arc::new(|_: &Invocation| {
            Ok(ExecutionControl {
                now_tick: 2,
                cancellation_requested: false,
            })
        }))
        .unwrap();
    router
        .bind_runtime(scoped.clone(), store.clone(), store.clone())
        .unwrap();
    let CompilerResult::Published { artifact, .. } = CobolCompiler::default()
        .compile(CompilerRequest {
            source: source_bundle(&super::super::hardening::input(SOURCE)).unwrap(),
            mode: CompilationMode::Executable,
            target: CompileTarget::new("reference").unwrap(),
            options: CompileOptions::new(BTreeMap::new()).unwrap(),
        })
        .unwrap()
    else {
        panic!("real compiled executable")
    };
    let record = artifact::published_artifact_record(&artifact).unwrap();
    let metadata = record.executable.as_ref().unwrap().clone();
    store.put_artifact(record.clone()).unwrap();
    let cics_store = if foreign_adapters {
        Arc::new(MemoryStore::new(Default::default()))
    } else {
        store.clone()
    };
    if foreign_adapters {
        cics_store.put_artifact(record.clone()).unwrap();
    }
    for physical in [&store, &cics_store] {
        for (namespace, key, payload) in [
            ("online-transaction", "TEST", b"CICSENTR".to_vec()),
            (
                "online-program",
                "CICSENTR",
                record.artifact.as_str().as_bytes().to_vec(),
            ),
        ] {
            if physical
                .get_provider_state(namespace, key)
                .unwrap()
                .is_none()
            {
                physical
                    .put_provider_state(
                        ProviderStateRecord {
                            namespace: namespace.into(),
                            key: key.into(),
                            version: 1,
                            payload,
                        },
                        None,
                    )
                    .unwrap();
            }
        }
    }
    let cics = CicsService::open(scoped, cics_store.clone(), CicsLimits::default()).unwrap();
    cics.bind_artifact_store(cics_store).unwrap();
    cics.register_program_definitions(&[CicsProgramDefinition {
        name: "CICSENTR".into(),
        generation: 1,
        artifact: record.artifact.clone(),
        semantic_identity: metadata.semantic_identity,
        entry_offset: 0,
        enabled: true,
        remote: false,
        reload: false,
        java_status: CicsJavaStatus::NotJava,
    }])
    .unwrap();
    let mut terminal = super::super::hardening::parent();
    terminal.selector = Selector::new("cics:TEST", Default::default()).unwrap();
    terminal.deadline_tick = 100;
    let grants = ["host.security.authorize", "host.program.invoke"]
        .into_iter()
        .map(|cap| CapabilityId::new(cap, Default::default()).unwrap())
        .collect();
    terminal.principal = Principal::new(
        PrincipalId::new("CICSUSER", Default::default()).unwrap(),
        grants,
        Default::default(),
    )
    .unwrap();
    let session = SessionId::new("compiled-initial-task", 64).unwrap();
    for (key, schema, bytes) in [
        (
            "cics.execution-context",
            "mainframe-env.cics.execution-context@1",
            b"local".to_vec(),
        ),
        (
            "cics.session",
            "mainframe-env.cics.session@1",
            session.as_str().as_bytes().to_vec(),
        ),
    ] {
        terminal.bindings.insert(
            key.into(),
            BoundedPayload::new(schema, bytes, Default::default()).unwrap(),
        );
    }
    cics.launch_terminal(
        terminal.clone(),
        &session,
        "TEST",
        24,
        80,
        "compiled-csrf",
        1,
        100,
    )
    .unwrap();
    // Independent exact ordinary initial-entry transformation, not product helpers.
    let mut actor = terminal.clone();
    actor.selector = Selector::new("program:CICSENTR", Default::default()).unwrap();
    actor.artifact = record.artifact;
    actor.idempotency_key = IdempotencyKey::new(
        format!("{}:CICSENTR", terminal.idempotency_key.as_str()),
        Default::default(),
    )
    .unwrap();
    for (key, schema, bytes) in [
        (
            "cics.program-entry",
            "mainframe-env.cics.program-entry@1",
            b"initial".to_vec(),
        ),
        ("cics.commarea", "mainframe-env.cics.commarea@1", vec![]),
        ("cics.aid", "mainframe-env.cics.aid@1", vec![0]),
        (
            "cics.transaction",
            "mainframe-env.cics.transaction@1",
            b"TEST".to_vec(),
        ),
    ] {
        actor.bindings.insert(
            key.into(),
            BoundedPayload::new(schema, bytes, Default::default()).unwrap(),
        );
    }
    for name in ["CEEDAYS", "COBDATFT", "MVSWAIT", "CEE3ABD"] {
        actor.bindings.insert(
            format!("cobol.runtime-service.{name}"),
            BoundedPayload::new(
                "mainframe-env.runtime-service-selector@1",
                format!("le:{name}:1").into_bytes(),
                Default::default(),
            )
            .unwrap(),
        );
    }
    Fixture {
        router,
        cics,
        store,
        terminal,
        actor,
        calls,
    }
}
struct Installed {
    mode: &'static str,
    admitted: usize,
    dispatched: usize,
    retained: usize,
    key: Option<IdempotencyKey>,
}
impl Installed {
    fn new(mode: &'static str) -> Self {
        Self {
            mode,
            admitted: 0,
            dispatched: 0,
            retained: 0,
            key: None,
        }
    }
}
impl CicsProtectedCompiledHost for Installed {
    fn configuration(
        &mut self,
        program: &CicsProtectedCompiledProgram<'_>,
    ) -> Result<ProtectedStorageConfiguration, HostProblem> {
        assert_eq!(program.program(), "CICSENTR");
        assert_eq!(program.artifact(), &program.original().artifact);
        assert!(program.metadata().validate());
        Ok(ProtectedStorageConfiguration {
            configuration_digest: *program.content_digest(),
            provider_namespaces: vec![],
            provider_rows: vec![],
        })
    }
    fn admit_initial_frame(
        &mut self,
        program: &CicsProtectedCompiledProgram<'_>,
        storage: &ProtectedStorageAdmission<'_>,
        task: &CicsCurrentProgramLoan<'_>,
        selection: &ProgramLinkSelection,
    ) -> Result<Option<Arc<dyn MqMqiProgramFrame>>, HostProblem> {
        self.admitted += 1;
        storage.require_current(program.original()).unwrap();
        task.require_current(program.original(), 2).unwrap();
        assert_eq!(
            storage.claim().admission().execution.state,
            ExecutionState::Admitted
        );
        assert_eq!(storage.claim().admission().execution.version, 1);
        assert_eq!(selection.artifact, *program.artifact());
        // This non-MQ compiled infrastructure control installs no MQ frame.
        Ok(None)
    }
    fn with_dispatch(
        &mut self,
        actual: &CicsProtectedCompiledDispatch<'_>,
        invoke: &mut dyn FnMut() -> Result<(), HostProblem>,
    ) -> Result<(), HostProblem> {
        self.dispatched += 1;
        actual.require_current().unwrap();
        assert_eq!(actual.dispatch().intent().state, EffectState::Intent);
        assert_eq!(actual.task().invocation(), actual.program().original());
        self.key = Some(actual.dispatch().intent().key.clone());
        match self.mode {
            "omit" => return Ok(()),
            "before" => return Err(HostProblem::UnknownOutcome),
            "unsupported" => return Err(HostProblem::Unsupported),
            _ => {}
        }
        invoke()?;
        if self.mode == "repeat" {
            assert_eq!(invoke(), Err(HostProblem::UnknownOutcome));
        }
        if self.mode == "panic" {
            panic!("actual held compiled task failure");
        }
        Ok(())
    }
    fn retain_uncertain(&mut self, _: &Invocation) -> Result<(), HostProblem> {
        self.retained += 1;
        Ok(())
    }
}
#[test]
fn actual_compiled_initial_cics_dispatch_borrows_same_task_storage_and_intent_without_settlement() {
    let f = fixture(false);
    let frozen = f.actor.clone();
    let mut installed = Installed::new("once");
    let outcome = f
        .router
        .execute_first_protected_cics_dispatch(&f.cics, &f.terminal, &f.actor, &mut installed)
        .unwrap();
    assert!(
        matches!(outcome, ExecutionOutcome::ProviderFailure(ref problem) if problem.has_unknown_outcome())
    );
    assert_eq!(f.actor, frozen);
    assert_eq!(
        (installed.admitted, installed.dispatched, installed.retained),
        (1, 1, 1)
    );
    assert_eq!(f.calls.load(Ordering::SeqCst), 1);
    let execution = f
        .store
        .get_execution(&f.actor.execution_id)
        .unwrap()
        .unwrap();
    assert_eq!(execution.state, ExecutionState::Running);
    assert!(execution.terminal_tick.is_none());
    let effect = f
        .store
        .effect(installed.key.as_ref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(effect.state, EffectState::UnknownOutcome);
    assert!(effect.result_digest.is_some());
    assert_eq!(effect.resolved_tick, None);
    let root = f
        .store
        .get_provider_state(ROOT_DRIVER_NAMESPACE, f.actor.execution_id.as_str())
        .unwrap()
        .unwrap();
    let document: serde_json::Value = serde_json::from_slice(&root.payload).unwrap();
    assert_eq!(document["phase"], 3);
    assert_eq!(
        f.cics.with_current_program_loan(&f.actor, 2, |_| Ok(())),
        Err(HostProblem::UnknownOutcome)
    );
}
#[test]
fn compiled_cics_dispatch_refuses_equal_foreign_adapters_changed_actors_and_default_port() {
    let f = fixture(true);
    let mut installed = Installed::new("once");
    let outcome = f
        .router
        .execute_first_protected_cics_dispatch(&f.cics, &f.terminal, &f.actor, &mut installed)
        .unwrap();
    assert!(
        matches!(outcome, ExecutionOutcome::ProviderFailure(ref problem) if problem.has_unknown_outcome())
    );
    assert_eq!((installed.admitted, installed.dispatched), (0, 0));
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    for mode in ["attempt", "key", "principal", "binding"] {
        let f = fixture(false);
        let mut changed = f.actor.clone();
        match mode {
            "attempt" => changed.attempt += 1,
            "key" => {
                changed.idempotency_key =
                    IdempotencyKey::new("foreign:CICSENTR", Default::default()).unwrap()
            }
            "principal" => {
                changed.principal = Principal::new(
                    PrincipalId::new("FOREIGN", Default::default()).unwrap(),
                    changed.principal.grants().clone(),
                    Default::default(),
                )
                .unwrap()
            }
            "binding" => {
                changed.bindings.remove("cics.aid");
            }
            _ => unreachable!(),
        }
        let mut installed = Installed::new("once");
        let outcome = f
            .router
            .execute_first_protected_cics_dispatch(&f.cics, &f.terminal, &changed, &mut installed)
            .unwrap();
        assert!(
            matches!(outcome, ExecutionOutcome::ProviderFailure(ref problem) if problem.has_unknown_outcome())
        );
        assert_eq!((installed.admitted, installed.dispatched), (0, 0));
        assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    }
    struct Unsupported;
    impl CicsProtectedCompiledHost for Unsupported {}
    let f = fixture(false);
    assert!(matches!(
        f.router.execute_first_protected_cics_dispatch(
            &f.cics,
            &f.terminal,
            &f.actor,
            &mut Unsupported
        ),
        Err(HostProblem::Unsupported)
    ));
    assert!(
        f.store
            .get_execution(&f.actor.execution_id)
            .unwrap()
            .is_none()
    );
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}
#[test]
fn compiled_cics_first_dispatch_refusal_panic_and_repeat_retain_one_original_intent() {
    for mode in ["omit", "before", "unsupported", "repeat", "panic"] {
        let f = fixture(false);
        let mut installed = Installed::new(mode);
        let outcome = f
            .router
            .execute_first_protected_cics_dispatch(&f.cics, &f.terminal, &f.actor, &mut installed)
            .unwrap();
        assert!(
            matches!(outcome, ExecutionOutcome::ProviderFailure(ref problem) if problem.has_unknown_outcome())
        );
        assert_eq!(
            (installed.admitted, installed.dispatched, installed.retained),
            (1, 1, 1)
        );
        assert_eq!(
            f.calls.load(Ordering::SeqCst),
            usize::from(matches!(mode, "repeat" | "panic"))
        );
        let effect = f
            .store
            .effect(installed.key.as_ref().unwrap())
            .unwrap()
            .unwrap();
        let called = matches!(mode, "repeat" | "panic");
        assert_eq!(
            effect.state,
            if called {
                EffectState::UnknownOutcome
            } else {
                EffectState::Intent
            }
        );
        assert_eq!(effect.result_digest.is_some(), called);
        assert_eq!(effect.resolved_tick, None);
        assert_eq!(
            f.store
                .get_execution(&f.actor.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Running
        );
        assert_eq!(
            f.cics.with_current_program_loan(&f.actor, 2, |_| Ok(())),
            Err(HostProblem::UnknownOutcome)
        );
    }
}
