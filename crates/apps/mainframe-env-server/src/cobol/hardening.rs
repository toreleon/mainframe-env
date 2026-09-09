//! Installed-program regressions. The fake effect provider commits real store
//! state; compiler, installation, host dispatch, interpreter and journal are real.
use super::*;
use mainframe_env_batch::DdPlan;
use mainframe_env_execution_api::{CapabilityId, PrincipalId, ResourceLimits, ServiceClass};
use mainframe_env_host_api::{HostLimits, ProgramName, RegistrySnapshot};
use mainframe_env_store::{LocalArtifactStore, MemoryStore, SqliteStateStore, StoreLimits};
use mainframe_env_store_api::EffectState;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Mutex, Weak};

const MIDDLE: &str = "IDENTIFICATION DIVISION.\nPROGRAM-ID. MIDDLE.\nPROCEDURE DIVISION.\nCALL 'EFFECT' ON EXCEPTION DISPLAY 'CAUGHT' END-CALL.\nGOBACK.\n";
const ROOT: &str = "IDENTIFICATION DIVISION.\nPROGRAM-ID. ROOT.\nPROCEDURE DIVISION.\nCALL 'MIDDLE' ON EXCEPTION DISPLAY 'WRONG-PARENT-SUCCESS' END-CALL.\nSTOP RUN.\n";
static NEXT_ROOT: AtomicU64 = AtomicU64::new(1);

pub(super) struct TestRoot(pub PathBuf);
impl TestRoot {
    pub(super) fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "mainframe-hardening-{}-{}-{}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn sqlite_url(&self) -> String {
        format!("sqlite://{}?mode=rwc", self.0.join("state.db").display())
    }
}
impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) fn parent() -> Invocation {
    let limits = InvocationLimits::default();
    Invocation::new(
        RequestId::new("parent-request", limits).unwrap(),
        ExecutionId::new("parent-execution", limits).unwrap(),
        RunUnitId::new("parent-run", limits).unwrap(),
        None,
        Selector::new("program:test", limits).unwrap(),
        ArtifactRef::new("artifact", limits).unwrap(),
        Principal::new(
            PrincipalId::new("BATCH", limits).unwrap(),
            BTreeSet::from([CapabilityId::new("host.program.invoke", limits).unwrap()]),
            limits,
        )
        .unwrap(),
        ServiceClass::Batch,
        0,
        u64::MAX,
        TraceId::new("parent-trace", limits).unwrap(),
        IdempotencyKey::new("parent-key", limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .unwrap()
}

pub(super) fn input(source: &str) -> ProgramInput {
    ProgramInput {
        parameter: None,
        execution: None,
        dd_records: BTreeMap::new(),
        dds: vec![DdPlan {
            name: "SYSIN".into(),
            dataset: None,
            member: None,
            generation: None,
            organization: None,
            record_format: None,
            logical_record_length: None,
            ccsid: None,
            temporary: false,
            sysout: None,
            disposition: Vec::new(),
            inline_data: source.as_bytes().to_vec(),
            concatenation: false,
            source_line: 1,
            source_end_line: 1,
            parameters: Vec::new(),
        }],
    }
}

pub(super) fn call_payload(values: &[Vec<u8>]) -> BoundedPayload {
    let mut bytes = u32::try_from(values.len()).unwrap().to_be_bytes().to_vec();
    for (index, value) in values.iter().enumerate() {
        let name = format!("ARG{}", index + 1);
        bytes.extend_from_slice(&(name.len() as u64).to_be_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
        bytes.extend_from_slice(value);
    }
    BoundedPayload::new(
        "mainframe-env.cobol.call@1",
        bytes,
        InvocationLimits::default(),
    )
    .unwrap()
}

struct FaultRouter {
    descriptor: CapabilityDescriptor,
    router: Weak<DefaultProgramRouter>,
    store: Arc<dyn PlatformStore>,
    problem: HostProblem,
    delete_cursors: bool,
    observed: Mutex<Vec<EffectRequest>>,
}

impl HostProvider for FaultRouter {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }
    fn invoke(&self, invocation: &Invocation, effect: EffectRequest) -> EffectResult {
        self.observed.lock().unwrap().push(effect.clone());
        if matches!(&effect.request, HostRequest::Program(ProgramRequest::Call { program, .. }) if program.as_str() == "EFFECT")
        {
            if self.problem == HostProblem::UnknownOutcome {
                let key = effect
                    .idempotency_key
                    .as_ref()
                    .expect("effect identity")
                    .as_str();
                if self
                    .store
                    .get_provider_state("hardening-49-business", key)
                    .unwrap()
                    .is_none()
                {
                    self.store
                        .put_provider_state(
                            ProviderStateRecord {
                                namespace: "hardening-49-business".into(),
                                key: key.into(),
                                version: 1,
                                payload: b"committed".to_vec(),
                            },
                            None,
                        )
                        .unwrap();
                }
            }
            if std::env::var_os("MAINFRAME_ENV_TEST_CRASH_AFTER_BUSINESS").is_some() {
                std::process::exit(55);
            }
            if self.delete_cursors {
                for record in self
                    .store
                    .list_provider_state("batch-file-cursor", 128)
                    .unwrap()
                {
                    self.store
                        .delete_provider_state(&record.namespace, &record.key, record.version)
                        .unwrap();
                }
            }
            return EffectResult {
                sequence: effect.sequence,
                outcome: Err(self.problem.clone()),
            };
        }
        self.router
            .upgrade()
            .expect("live installed-program router")
            .invoke(invocation, effect)
    }
}

pub(super) struct Fixture {
    pub(super) store: Arc<dyn PlatformStore>,
    pub(super) host: Arc<ScopedHostService>,
    pub(super) router: Arc<DefaultProgramRouter>,
    root: PathBuf,
    faults: Arc<FaultRouter>,
}

impl Fixture {
    pub(super) fn new(
        root: &TestRoot,
        store: Arc<dyn PlatformStore>,
        problem: HostProblem,
        delete_cursors: bool,
    ) -> Self {
        let router = default_program_router();
        let faults = Arc::new(FaultRouter {
            descriptor: router.descriptor().clone(),
            router: Arc::downgrade(&router),
            store: store.clone(),
            problem,
            delete_cursors,
            observed: Mutex::new(Vec::new()),
        });
        let host = Arc::new(ScopedHostService::new(
            Arc::new(
                RegistrySnapshot::new(
                    1,
                    vec![faults.clone() as Arc<dyn HostProvider>],
                    InvocationLimits::default(),
                )
                .unwrap(),
            ),
            HostLimits::default(),
        ));
        let artifacts = Arc::new(LocalArtifactStore::open(&root.0, 64 * 1024 * 1024).unwrap());
        router
            .bind_runtime(host.clone(), store.clone(), artifacts)
            .unwrap();
        Self {
            store,
            host,
            router,
            root: root.0.clone(),
            faults,
        }
    }

    pub(super) fn install(&self, name: &str, source: &str) {
        let CompilerResult::Published { artifact, .. } = CobolCompiler::default()
            .compile(CompilerRequest {
                source: source_bundle(&input(source)).unwrap(),
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").unwrap(),
                options: CompileOptions::new(BTreeMap::new()).unwrap(),
            })
            .unwrap()
        else {
            panic!("executable artifact required");
        };
        let record = artifact::published_artifact_record(&artifact).unwrap();
        let id = record.artifact.clone();
        LocalArtifactStore::open(&self.root, 64 * 1024 * 1024)
            .unwrap()
            .put_artifact(record)
            .unwrap();
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

    pub(super) fn call(
        &self,
        parent: &Invocation,
        program: &str,
        sequence: u64,
        payload: BoundedPayload,
    ) -> EffectResult {
        // Keep the router alive without a router -> host -> router Arc cycle.
        assert!(Arc::strong_count(&self.router) >= 1);
        self.host
            .invoke(
                parent,
                0,
                false,
                EffectRequest {
                    run_unit: parent.run_unit_id.clone(),
                    sequence,
                    deadline_tick: parent.deadline_tick,
                    idempotency_key: Some(
                        IdempotencyKey::new(
                            format!("{}:{sequence}", parent.idempotency_key),
                            InvocationLimits::default(),
                        )
                        .unwrap(),
                    ),
                    request: HostRequest::Program(ProgramRequest::Call {
                        program: ProgramName::new(program, 128).unwrap(),
                        payload,
                        service: None,
                    }),
                },
            )
            .persist_with(|audit| {
                self.store
                    .record_audit(audit)
                    .map_err(|_| HostProblem::InfrastructureFailure)
            })
    }

    fn batch(&self, program: &str, source: &str) -> EffectResult {
        self.call(
            &parent(),
            program,
            1,
            BoundedPayload::new(
                "mainframe-env.program.input@1",
                serde_json::to_vec(&input(source)).unwrap(),
                InvocationLimits::default(),
            )
            .unwrap(),
        )
    }
}

#[test]
fn hardening_49_unknown_effect_survives_all_cobol_adapters_and_store_reopen() {
    for profile in ["call", "installed-batch", "inline"] {
        let root = TestRoot::new();
        let url = root.sqlite_url();
        let store: Arc<dyn PlatformStore> =
            Arc::new(SqliteStateStore::open(&url, 8 * 1024 * 1024, 65536).unwrap());
        let fixture = Fixture::new(&root, store, HostProblem::UnknownOutcome, false);
        fixture.install("MIDDLE", MIDDLE);
        fixture.install("ROOT", ROOT);
        let result = match profile {
            "call" => fixture.call(&parent(), "ROOT", 1, call_payload(&[])),
            "installed-batch" => fixture.batch("ROOT", ""),
            "inline" => fixture.batch("COBOL", ROOT),
            _ => unreachable!(),
        };
        assert_eq!(
            result.outcome,
            Err(HostProblem::UnknownOutcome),
            "{profile}"
        );
        let business = fixture
            .store
            .list_provider_state("hardening-49-business", 128)
            .unwrap();
        assert_eq!(
            business.len(),
            1,
            "a real business effect was committed before uncertainty"
        );
        let receipts = fixture.store.unknown_effects(128).unwrap();
        assert!(!receipts.is_empty(), "every adapter retains uncertainty");
        for receipt in &receipts {
            assert_eq!(receipt.state, EffectState::UnknownOutcome);
            assert_eq!(
                receipt.digest_format,
                mainframe_env_store_api::EffectDigestFormat::CanonicalHostV1
            );
            assert_eq!(
                receipt.result_digest,
                Some(
                    mainframe_env_host_api::canonical_result_digest(&Err(
                        HostProblem::UnknownOutcome
                    ))
                    .unwrap()
                )
            );
        }
        drop(fixture);
        let reopened = SqliteStateStore::open(&url, 8 * 1024 * 1024, 65536).unwrap();
        use mainframe_env_store_api::{IdempotencyStore, ProviderStateStore};
        assert_eq!(
            reopened.unknown_effects(128).unwrap(),
            receipts,
            "reopen preserves execution/run-unit/sequence/key and request/result digests"
        );
        assert_eq!(
            reopened
                .list_provider_state("hardening-49-business", 128)
                .unwrap(),
            business
        );
        for receipt in receipts {
            // Explicit operator reconciliation, not an implicit retry of work.
            let reconciled = reopened
                .reconcile_unknown_versioned(
                    &receipt.key,
                    EffectState::Completed,
                    receipt.digest_format,
                    [49; 32],
                )
                .unwrap();
            assert_eq!(reconciled.key, receipt.key);
            assert_eq!(reconciled.state, EffectState::Completed);
        }
        assert!(reopened.unknown_effects(128).unwrap().is_empty());
    }
}

#[test]
fn hardening_49_known_rejection_remains_a_handleable_call_exception() {
    let root = TestRoot::new();
    let fixture = Fixture::new(
        &root,
        Arc::new(MemoryStore::new(Default::default())),
        HostProblem::Condition {
            name: "KNOWN-REJECTION".into(),
            response: 9,
            response2: 0,
        },
        false,
    );
    fixture.install("MIDDLE", MIDDLE);
    let HostResult::Program(payload) = fixture.batch("MIDDLE", "").outcome.unwrap() else {
        panic!("program output");
    };
    let output: ProgramOutput = serde_json::from_slice(payload.bytes()).unwrap();
    assert_eq!(output.return_code, 0);
    assert_eq!(output.records, vec![b"CAUGHT".to_vec()]);
    assert!(
        fixture
            .store
            .list_provider_state("hardening-49-business", 128)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn hardening_49_secondary_cursor_failure_does_not_erase_uncertainty() {
    let root = TestRoot::new();
    let fixture = Fixture::new(
        &root,
        Arc::new(MemoryStore::new(Default::default())),
        HostProblem::UnknownOutcome,
        true,
    );
    fixture.install("MIDDLE", MIDDLE);
    fixture.install("ROOT", ROOT);
    fixture
        .store
        .put_provider_state(
            ProviderStateRecord {
                namespace: "batch-file-cursor".into(),
                key: "parent-run:ROOT".into(),
                version: 1,
                payload: b"{}".to_vec(),
            },
            None,
        )
        .unwrap();
    let result = fixture.call(&parent(), "ROOT", 1, call_payload(&[]));
    assert_eq!(result.outcome, Err(HostProblem::UnknownOutcome));
    assert!(
        fixture
            .store
            .get_provider_state("batch-file-cursor", "parent-run:ROOT")
            .unwrap()
            .is_none()
    );
}

#[test]
fn hardening_49_journal_result_and_terminal_failure_do_not_erase_uncertainty() {
    for max_events in [4, 5] {
        let root = TestRoot::new();
        let fixture = Fixture::new(
            &root,
            Arc::new(MemoryStore::new(StoreLimits {
                max_events,
                ..Default::default()
            })),
            HostProblem::UnknownOutcome,
            false,
        );
        fixture.install("MIDDLE", MIDDLE);
        fixture.install("ROOT", ROOT);
        assert_eq!(
            fixture.batch("MIDDLE", "").outcome,
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(
            fixture
                .store
                .list_provider_state("hardening-49-business", 128)
                .unwrap()
                .len(),
            1
        );
        let calls = fixture.faults.observed.lock().unwrap();
        let middle = calls.iter().find(|effect| matches!(&effect.request,
            HostRequest::Program(ProgramRequest::Call { program, .. }) if program.as_str() == "EFFECT")).unwrap();
        let receipt = fixture
            .store
            .effect(middle.idempotency_key.as_ref().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(
            receipt.state,
            if max_events == 4 {
                EffectState::Intent
            } else {
                EffectState::UnknownOutcome
            }
        );
    }
}

const CPU_LOOP: &str = "IDENTIFICATION DIVISION.\nPROGRAM-ID. CPU-LOOP.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 COUNTER PIC 9(8) VALUE ZERO.\nPROCEDURE DIVISION.\nPERFORM UNTIL COUNTER = 99999999\n  ADD 1 TO COUNTER\nEND-PERFORM.\nGOBACK.\n";

#[test]
fn hardening_48_cpu_only_installed_inline_and_nested_programs_observe_live_controls() {
    for profile in ["call", "installed-batch", "inline", "nested"] {
        for cancel in [true, false] {
            let root = TestRoot::new();
            let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(StoreLimits::default()));
            let fixture = Fixture::new(&root, store, HostProblem::NotFound, false);
            fixture.install("CPU-LOOP", CPU_LOOP);
            let nested = "IDENTIFICATION DIVISION.\nPROGRAM-ID. OUTER.\nPROCEDURE DIVISION.\nCALL 'CPU-LOOP'.\nSTOP RUN.\n";
            fixture.install("OUTER", nested);
            let observations = Arc::new(AtomicU64::new(0));
            let observed = observations.clone();
            fixture
                .router
                .bind_execution_control(Arc::new(move |inv: &Invocation| {
                    if inv.selector.as_str() == "program:CPU-LOOP"
                        || inv.selector.as_str() == "program:COBOL"
                    {
                        let n = observed.fetch_add(1, Ordering::SeqCst);
                        Ok(ExecutionControl {
                            now_tick: if n == 0 { 0 } else { inv.deadline_tick },
                            cancellation_requested: cancel && n > 0,
                        })
                    } else {
                        Ok(ExecutionControl::default())
                    }
                }))
                .unwrap();
            let mut inv = parent();
            inv.deadline_tick = 100;
            let payload = if matches!(profile, "call" | "nested") {
                call_payload(&[])
            } else {
                BoundedPayload::new(
                    "mainframe-env.program.input@1",
                    serde_json::to_vec(&input(if profile == "inline" { CPU_LOOP } else { "" }))
                        .unwrap(),
                    InvocationLimits::default(),
                )
                .unwrap()
            };
            let name = match profile {
                "nested" => "OUTER",
                "inline" => "COBOL",
                _ => "CPU-LOOP",
            };
            let result = fixture.call(&inv, name, 1, payload);
            assert_eq!(
                result.outcome,
                Err(if cancel {
                    HostProblem::Cancelled
                } else {
                    HostProblem::TimedOut
                }),
                "{profile}"
            );
            assert_eq!(
                observations.load(Ordering::SeqCst),
                2,
                "stop after first CPU quantum: {profile}"
            );
            assert!(fixture.faults.observed.lock().unwrap().iter().all(|effect|
                !matches!(&effect.request, HostRequest::Program(ProgramRequest::Call { program, .. }) if program.as_str() == "EFFECT")));
        }
    }
}

fn queued_control_work(inv: &Invocation, id: &str) -> mainframe_env_store_api::WorkRecord {
    mainframe_env_store_api::WorkRecord {
        work_id: id.into(),
        execution_id: inv.execution_id.clone(),
        required_selector: inv.selector.clone(),
        required_generation: "test-control@1".into(),
        artifact: inv.artifact.clone(),
        state: mainframe_env_store_api::WorkState::Queued,
        priority: 0,
        attempt: 0,
        max_attempts: 3,
        available_tick: 0,
        deadline_tick: u64::MAX,
        cancellation_requested: false,
        worker_id: None,
        lease_id: None,
        lease_epoch: 0,
        lease_expiry_tick: None,
        heartbeat_tick: None,
        terminal_tick: None,
        checkpoint_id: None,
        effect_sequence: 0,
        payload: vec![],
    }
}

#[test]
fn hardening_48_durable_job_cancel_is_observed_inside_cpu_only_cobol_and_is_scoped() {
    for sqlite in [false, true] {
        let root = TestRoot::new();
        let store: Arc<dyn PlatformStore> = if sqlite {
            Arc::new(SqliteStateStore::open(&root.sqlite_url(), 8 * 1024 * 1024, 65536).unwrap())
        } else {
            Arc::new(MemoryStore::new(StoreLimits::default()))
        };
        let fixture = Fixture::new(&root, store.clone(), HostProblem::NotFound, false);
        fixture.install("CPU-LOOP", CPU_LOOP);
        let mut inv = parent();
        let id = "jes:CONTROL-JOB";
        store.enqueue(queued_control_work(&inv, id)).unwrap();
        store
            .enqueue(queued_control_work(&inv, "jes:OTHER-JOB"))
            .unwrap();
        inv.bindings.insert(
            "jes.work-id".into(),
            BoundedPayload::new(
                "mainframe-env.jes-work@1",
                id.as_bytes().to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
        let observations = Arc::new(AtomicU64::new(0));
        let observed = observations.clone();
        let observed_store = store.clone();
        fixture
            .router
            .bind_execution_control(Arc::new(move |_: &Invocation| {
                if observed.fetch_add(1, Ordering::SeqCst) == 1 {
                    // This is the same durable operation used by ProductServer JobCancel.
                    observed_store.request_cancellation(id).unwrap();
                }
                Ok(ExecutionControl::default())
            }))
            .unwrap();
        assert_eq!(
            fixture.call(&inv, "CPU-LOOP", 1, call_payload(&[])).outcome,
            Err(HostProblem::Cancelled)
        );
        assert_eq!(observations.load(Ordering::SeqCst), 2);
        assert!(store.get_work(id).unwrap().unwrap().cancellation_requested);
        assert!(
            !store
                .get_work("jes:OTHER-JOB")
                .unwrap()
                .unwrap()
                .cancellation_requested
        );
        inv.bindings.insert(
            "jes.work-id".into(),
            BoundedPayload::new(
                "mainframe-env.jes-work@1",
                b"jes:OTHER-JOB".to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
        assert!(
            !fixture
                .router
                .observe_execution_control(&inv)
                .unwrap()
                .cancellation_requested
        );
    }
}

#[test]
fn hardening_48_unknown_host_outcome_wins_over_new_cancellation() {
    let root = TestRoot::new();
    let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(StoreLimits::default()));
    let fixture = Fixture::new(&root, store.clone(), HostProblem::UnknownOutcome, false);
    fixture.install("MIDDLE", MIDDLE);
    fixture.install("ROOT", ROOT);
    let observed_store = store.clone();
    fixture
        .router
        .bind_execution_control(Arc::new(move |_: &Invocation| {
            Ok(ExecutionControl {
                now_tick: 0,
                cancellation_requested: !observed_store
                    .list_provider_state("hardening-49-business", 10)
                    .unwrap()
                    .is_empty(),
            })
        }))
        .unwrap();
    assert_eq!(
        fixture.batch("ROOT", "").outcome,
        Err(HostProblem::UnknownOutcome)
    );
    assert_eq!(
        store
            .list_provider_state("hardening-49-business", 10)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn hardening_48_explicit_parent_cancellation_is_not_lost_in_child_construction() {
    let root = TestRoot::new();
    let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(StoreLimits::default()));
    let fixture = Fixture::new(&root, store, HostProblem::NotFound, false);
    fixture.install("CPU-LOOP", CPU_LOOP);
    let mut inv = parent();
    inv.cancellation = Some(mainframe_env_execution_api::Cancellation {
        id: mainframe_env_execution_api::CancellationId::new("cancel", InvocationLimits::default())
            .unwrap(),
        reason: "requested".into(),
        requested_at_tick: 1,
    });
    // Direct adapter entry, bypassing an outer host admission check, exercises propagation.
    assert_eq!(
        fixture.router.cobol.execute_installed(
            &inv,
            "CPU-LOOP",
            &call_payload(&[]),
            "control-test",
            &mut Vec::new(),
        ),
        Err(HostProblem::Cancelled)
    );
}

const REPLAY_LEAF: &str =
    "IDENTIFICATION DIVISION.\nPROGRAM-ID. REPLAY-LEAF.\nPROCEDURE DIVISION.\nGOBACK.\n";
const REPLAY_MIDDLE: &str = "IDENTIFICATION DIVISION.\nPROGRAM-ID. REPLAY-MIDDLE.\nPROCEDURE DIVISION.\nCALL 'REPLAY-LEAF'.\nCALL 'REPLAY-LEAF'.\nGOBACK.\n";
const REPLAY_ROOT: &str = "IDENTIFICATION DIVISION.\nPROGRAM-ID. REPLAY-ROOT.\nPROCEDURE DIVISION.\nCALL 'REPLAY-MIDDLE'.\nCALL 'REPLAY-MIDDLE'.\nGOBACK.\n";

#[test]
fn hardening_55_completed_nested_calls_replay_after_sqlite_reopen() {
    for batch in [false, true] {
        let root = TestRoot::new();
        let url = root.sqlite_url();
        let fixture = Fixture::new(
            &root,
            Arc::new(SqliteStateStore::open(&url, 8 * 1024 * 1024, 65536).unwrap()),
            HostProblem::NotFound,
            false,
        );
        fixture.install("REPLAY-LEAF", REPLAY_LEAF);
        fixture.install("REPLAY-MIDDLE", REPLAY_MIDDLE);
        fixture.install("REPLAY-ROOT", REPLAY_ROOT);
        let payload = if batch {
            BoundedPayload::new(
                "mainframe-env.program.input@1",
                serde_json::to_vec(&input("")).unwrap(),
                InvocationLimits::default(),
            )
            .unwrap()
        } else {
            call_payload(&[])
        };
        let first = fixture.call(&parent(), "REPLAY-ROOT", 1, payload.clone());
        assert!(first.outcome.is_ok(), "{first:?}");
        let receipts = fixture
            .store
            .list_provider_state("cobol-call-replay@1", 128)
            .unwrap();
        assert_eq!(
            receipts.len(),
            7,
            "root + two middle + four leaf call occurrences"
        );
        let executions: Vec<_> = receipts
            .iter()
            .map(|r| {
                assert_eq!(r.version, 2);
                let value: serde_json::Value = serde_json::from_slice(&r.payload).unwrap();
                let id = ExecutionId::new(
                    value["child_execution"].as_str().unwrap(),
                    InvocationLimits::default(),
                )
                .unwrap();
                let execution = fixture.store.get_execution(&id).unwrap().unwrap();
                assert_eq!(execution.run_unit_id, parent().run_unit_id);
                (id, execution)
            })
            .collect();
        drop(fixture);
        let reopened = Fixture::new(
            &root,
            Arc::new(SqliteStateStore::open(&url, 8 * 1024 * 1024, 65536).unwrap()),
            HostProblem::NotFound,
            false,
        );
        reopened
            .router
            .cobol
            .sequence
            .store(12345, Ordering::SeqCst);
        let mut unrelated = parent();
        unrelated.run_unit_id =
            RunUnitId::new("unrelated-run", InvocationLimits::default()).unwrap();
        assert!(
            reopened
                .call(&unrelated, "REPLAY-LEAF", 999, call_payload(&[]))
                .outcome
                .is_ok()
        );
        let before = reopened
            .store
            .list_provider_state("cobol-call-replay@1", 128)
            .unwrap();
        let mut retry = parent();
        retry.attempt = 2;
        assert_eq!(
            reopened.call(&retry, "REPLAY-ROOT", 1, payload).outcome,
            first.outcome
        );
        assert_eq!(
            reopened
                .store
                .list_provider_state("cobol-call-replay@1", 128)
                .unwrap(),
            before
        );
        for (id, record) in executions {
            assert_eq!(reopened.store.get_execution(&id).unwrap(), Some(record));
        }
    }
}

// Invoked in a dedicated subprocess by the crash test. Never mutates parent env.
#[test]
fn hardening_55_crash_worker() {
    let Some(path) = std::env::var_os("MAINFRAME_ENV_TEST_CRASH_ROOT") else {
        return;
    };
    let root = TestRoot(PathBuf::from(path));
    let fixture = Fixture::new(
        &root,
        Arc::new(SqliteStateStore::open(&root.sqlite_url(), 8 * 1024 * 1024, 65536).unwrap()),
        HostProblem::UnknownOutcome,
        false,
    );
    fixture.install("MIDDLE", MIDDLE);
    fixture.install("ROOT", ROOT);
    let _ = fixture.call(&parent(), "ROOT", 1, call_payload(&[]));
    panic!("worker must exit at the committed business effect before returning a result");
}

#[test]
fn hardening_55_abrupt_exit_after_business_commit_does_not_repeat_effect() {
    let root = TestRoot::new();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "cobol::hardening::hardening_55_crash_worker",
            "--nocapture",
        ])
        .env("MAINFRAME_ENV_TEST_CRASH_ROOT", &root.0)
        .env("MAINFRAME_ENV_TEST_CRASH_AFTER_BUSINESS", "1")
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(55),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let fixture = Fixture::new(
        &root,
        Arc::new(SqliteStateStore::open(&root.sqlite_url(), 8 * 1024 * 1024, 65536).unwrap()),
        HostProblem::UnknownOutcome,
        false,
    );
    let before = fixture
        .store
        .list_provider_state("hardening-49-business", 128)
        .unwrap();
    assert_eq!(before.len(), 1);
    fixture.router.cobol.sequence.store(98765, Ordering::SeqCst);
    fixture.install("REPLAY-LEAF", REPLAY_LEAF);
    let mut unrelated = parent();
    unrelated.execution_id = ExecutionId::new("unrelated", InvocationLimits::default()).unwrap();
    unrelated.run_unit_id = RunUnitId::new("unrelated-run", InvocationLimits::default()).unwrap();
    assert!(
        fixture
            .call(&unrelated, "REPLAY-LEAF", 1, call_payload(&[]))
            .outcome
            .is_ok()
    );
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| fixture.call(&parent(), "ROOT", 1, call_payload(&[]))))
            .collect();
        for handle in handles {
            assert_eq!(
                handle.join().unwrap().outcome,
                Err(HostProblem::UnknownOutcome)
            );
        }
    });
    assert_eq!(
        fixture
            .store
            .list_provider_state("hardening-49-business", 128)
            .unwrap(),
        before
    );
    assert!(fixture.faults.observed.lock().unwrap().iter().all(|e|
        !matches!(&e.request,HostRequest::Program(ProgramRequest::Call{program,..}) if program.as_str()=="EFFECT")));
}

#[test]
fn hardening_55_concurrent_first_dispatch_has_one_durable_child() {
    let root = TestRoot::new();
    let fixture = Fixture::new(
        &root,
        Arc::new(MemoryStore::new(Default::default())),
        HostProblem::NotFound,
        false,
    );
    fixture.install("REPLAY-LEAF", REPLAY_LEAF);
    let barrier = std::sync::Barrier::new(4);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    fixture.call(&parent(), "REPLAY-LEAF", 1, call_payload(&[]))
                })
            })
            .collect();
        let mut completed = 0;
        for handle in handles {
            match handle.join().unwrap().outcome {
                Ok(HostResult::Program(_)) => completed += 1,
                Err(HostProblem::UnknownOutcome) => {}
                other => panic!("unexpected {other:?}"),
            }
        }
        assert!(completed >= 1);
    });
    let receipts = fixture
        .store
        .list_provider_state("cobol-call-replay@1", 128)
        .unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].version, 2);
}

#[test]
fn hardening_55_conflicting_inputs_and_legacy_retry_fail_closed() {
    let root = TestRoot::new();
    let fixture = Fixture::new(
        &root,
        Arc::new(MemoryStore::new(Default::default())),
        HostProblem::NotFound,
        false,
    );
    fixture.install("REPLAY-LEAF", REPLAY_LEAF);
    let mut legacy = parent();
    legacy.attempt = 2;
    assert_eq!(
        fixture
            .call(&legacy, "REPLAY-LEAF", 1, call_payload(&[]))
            .outcome,
        Err(HostProblem::UnknownOutcome)
    );
    assert!(
        fixture
            .call(&parent(), "REPLAY-LEAF", 1, call_payload(&[]))
            .outcome
            .is_ok()
    );
    assert_eq!(
        fixture
            .call(&parent(), "OTHER", 1, call_payload(&[]))
            .outcome,
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(
        fixture
            .call(&parent(), "REPLAY-LEAF", 1, call_payload(&[vec![1]]))
            .outcome,
        Err(HostProblem::IdempotencyConflict)
    );
    let mut other = parent();
    other.execution_id = ExecutionId::new("other-parent", InvocationLimits::default()).unwrap();
    other.run_unit_id = RunUnitId::new("other-run", InvocationLimits::default()).unwrap();
    assert!(
        fixture
            .call(&other, "REPLAY-LEAF", 1, call_payload(&[]))
            .outcome
            .is_ok()
    );
    assert_eq!(
        fixture
            .store
            .list_provider_state("cobol-call-replay@1", 128)
            .unwrap()
            .len(),
        2
    );
}

const INSTANCE_COUNTER: &str = "IDENTIFICATION DIVISION.\nPROGRAM-ID. COUNTER.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 N PIC 9 VALUE 0.\nLINKAGE SECTION.\n01 OUT-N PIC 9.\nPROCEDURE DIVISION USING OUT-N.\nADD 1 TO N.\nMOVE N TO OUT-N.\nGOBACK.\n";

fn counter_call(fixture: &Fixture, inv: &Invocation, sequence: u64, expected: u8) {
    assert_eq!(
        fixture
            .call(inv, "COUNTER", sequence, call_payload(&[vec![b'0']]))
            .outcome,
        Ok(HostResult::Program(
            encode_cobol_call_result(&[vec![expected]]).unwrap()
        ))
    );
}

#[test]
fn hardening_47_installed_counter_retains_working_storage_and_resets_local_linkage() {
    let root = TestRoot::new();
    let fixture = Fixture::new(
        &root,
        Arc::new(MemoryStore::new(Default::default())),
        HostProblem::NotFound,
        false,
    );
    fixture.install("COUNTER", INSTANCE_COUNTER);
    counter_call(&fixture, &parent(), 1, b'1');
    counter_call(&fixture, &parent(), 2, b'2');
    // Replaying occurrence 1 must not advance or rewind the last-used instance.
    counter_call(&fixture, &parent(), 1, b'1');
    counter_call(&fixture, &parent(), 3, b'3');
    let source = "IDENTIFICATION DIVISION.\nPROGRAM-ID. LOCAL-CHECK.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 W PIC 9 VALUE 0.\nLOCAL-STORAGE SECTION.\n01 L PIC 9 VALUE 0.\nLINKAGE SECTION.\n01 OUT-W PIC 9.\n01 OUT-L PIC 9.\n01 INPUT-N PIC 9.\nPROCEDURE DIVISION USING OUT-W OUT-L INPUT-N.\nADD 1 TO W.\nADD 1 TO L.\nMOVE W TO OUT-W.\nMOVE L TO OUT-L.\nADD 1 TO INPUT-N.\nGOBACK.\n";
    fixture.install("LOCAL-CHECK", source);
    for (seq, input, expected) in [(10, b'5', *b"116"), (11, b'8', *b"219")] {
        assert_eq!(
            fixture
                .call(
                    &parent(),
                    "LOCAL-CHECK",
                    seq,
                    call_payload(&[vec![b'0'], vec![b'0'], vec![input]])
                )
                .outcome,
            Ok(HostResult::Program(
                encode_cobol_call_result(&expected.map(|v| vec![v])).unwrap()
            ))
        );
    }
}

#[test]
fn hardening_47_initial_and_cancel_have_distinct_real_call_lifecycles() {
    for initial in [false, true] {
        let root = TestRoot::new();
        let fixture = Fixture::new(
            &root,
            Arc::new(MemoryStore::new(Default::default())),
            HostProblem::NotFound,
            false,
        );
        fixture.install(
            "COUNTER",
            &if initial {
                INSTANCE_COUNTER.replace(
                    "PROGRAM-ID. COUNTER.",
                    "PROGRAM-ID. COUNTER IS INITIAL PROGRAM.",
                )
            } else {
                INSTANCE_COUNTER.into()
            },
        );
        let caller = "IDENTIFICATION DIVISION.\nPROGRAM-ID. CALLER.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 R PIC 9.\nPROCEDURE DIVISION.\nCALL 'COUNTER' USING R.\nDISPLAY R.\nCALL 'COUNTER' USING R.\nDISPLAY R.\nCANCEL 'COUNTER'.\nCALL 'COUNTER' USING R.\nDISPLAY R.\nSTOP RUN.\n";
        let result = fixture.batch("COBOL", caller).outcome.unwrap();
        let HostResult::Program(payload) = result else {
            panic!("program output");
        };
        let output: ProgramOutput = serde_json::from_slice(payload.bytes()).unwrap();
        assert_eq!(
            output.records,
            if initial {
                vec![b"1".to_vec(), b"1".to_vec(), b"1".to_vec()]
            } else {
                vec![b"1".to_vec(), b"2".to_vec(), b"1".to_vec()]
            }
        );
    }
}

#[test]
fn hardening_47_cancel_replay_does_not_reset_a_later_call_and_run_end_is_terminal() {
    let root = TestRoot::new();
    let fixture = Fixture::new(
        &root,
        Arc::new(MemoryStore::new(Default::default())),
        HostProblem::NotFound,
        false,
    );
    fixture.install("COUNTER", INSTANCE_COUNTER);
    counter_call(&fixture, &parent(), 1, b'1');
    let effect = EffectRequest {
        run_unit: parent().run_unit_id,
        sequence: 2,
        deadline_tick: u64::MAX,
        idempotency_key: Some(
            IdempotencyKey::new("cancel-counter", InvocationLimits::default()).unwrap(),
        ),
        request: HostRequest::Program(ProgramRequest::Cancel {
            programs: vec![ProgramName::new("COUNTER", 128).unwrap()],
        }),
    };
    assert!(
        fixture
            .router
            .invoke(&parent(), effect.clone())
            .outcome
            .is_ok()
    );
    counter_call(&fixture, &parent(), 3, b'1');
    assert!(fixture.router.invoke(&parent(), effect).outcome.is_ok());
    counter_call(&fixture, &parent(), 4, b'2');
    fixture.router.finish_run_unit(&parent()).unwrap();
    fixture.router.finish_run_unit(&parent()).unwrap();
    assert!(
        matches!(fixture.call(&parent(), "COUNTER", 5, call_payload(&[vec![b'0']])).outcome,
        Err(HostProblem::Condition { name, .. }) if name == "COBOL-RUN-ENDED")
    );
    // A terminal boundary frees last-used storage, not the response receipts.
    counter_call(&fixture, &parent(), 4, b'2');
}

#[test]
fn hardening_47_nested_calls_and_concurrent_run_units_are_isolated() {
    let root = TestRoot::new();
    let fixture = Fixture::new(
        &root,
        Arc::new(MemoryStore::new(Default::default())),
        HostProblem::NotFound,
        false,
    );
    fixture.install("COUNTER", INSTANCE_COUNTER);
    fixture.install("WRAPPER", "IDENTIFICATION DIVISION.\nPROGRAM-ID. WRAPPER.\nDATA DIVISION.\nLINKAGE SECTION.\n01 R PIC 9.\nPROCEDURE DIVISION USING R.\nCALL 'COUNTER' USING R.\nGOBACK.\n");
    std::thread::scope(|scope| {
        for n in 0..4 {
            let fixture = &fixture;
            scope.spawn(move || {
                let mut inv = parent();
                inv.execution_id =
                    ExecutionId::new(format!("concurrent-{n}"), InvocationLimits::default())
                        .unwrap();
                inv.run_unit_id =
                    RunUnitId::new(format!("concurrent-run-{n}"), InvocationLimits::default())
                        .unwrap();
                for seq in 1..=2 {
                    assert_eq!(
                        fixture
                            .call(&inv, "WRAPPER", seq, call_payload(&[vec![b'0']]))
                            .outcome,
                        Ok(HostResult::Program(
                            encode_cobol_call_result(&[vec![b'0' + seq as u8]]).unwrap()
                        ))
                    );
                }
                fixture.router.finish_run_unit(&inv).unwrap();
            });
        }
    });
    counter_call(&fixture, &parent(), 1, b'1');
}

#[test]
fn hardening_47_last_used_state_and_cancel_survive_sqlite_reopen() {
    let root = TestRoot::new();
    let url = root.sqlite_url();
    let fixture = Fixture::new(
        &root,
        Arc::new(SqliteStateStore::open(&url, 8 * 1024 * 1024, 65536).unwrap()),
        HostProblem::NotFound,
        false,
    );
    fixture.install("COUNTER", INSTANCE_COUNTER);
    counter_call(&fixture, &parent(), 1, b'1');
    drop(fixture);
    let fixture = Fixture::new(
        &root,
        Arc::new(SqliteStateStore::open(&url, 8 * 1024 * 1024, 65536).unwrap()),
        HostProblem::NotFound,
        false,
    );
    counter_call(&fixture, &parent(), 1, b'1');
    counter_call(&fixture, &parent(), 2, b'2');
    fixture.router.finish_run_unit(&parent()).unwrap();
}

#[test]
fn hardening_47_unsupported_lifecycles_fail_closed_and_active_instance_cannot_be_cancelled() {
    for declaration in [
        "PROGRAM-ID. COUNTER IS RECURSIVE PROGRAM.",
        "PROGRAM-ID. COUNTER IS COMMON PROGRAM.",
    ] {
        let root = TestRoot::new();
        let fixture = Fixture::new(
            &root,
            Arc::new(MemoryStore::new(Default::default())),
            HostProblem::NotFound,
            false,
        );
        fixture.install(
            "COUNTER",
            &INSTANCE_COUNTER.replace("PROGRAM-ID. COUNTER.", declaration),
        );
        assert_eq!(
            fixture
                .call(&parent(), "COUNTER", 1, call_payload(&[vec![b'0']]))
                .outcome,
            Err(HostProblem::Unsupported)
        );
    }
    let root = TestRoot::new();
    let fixture = Fixture::new(
        &root,
        Arc::new(MemoryStore::new(Default::default())),
        HostProblem::UnknownOutcome,
        false,
    );
    fixture.install("MIDDLE", MIDDLE);
    assert_eq!(
        fixture
            .call(&parent(), "MIDDLE", 1, call_payload(&[]))
            .outcome,
        Err(HostProblem::UnknownOutcome)
    );
    let cancel = EffectRequest {
        run_unit: parent().run_unit_id,
        sequence: 2,
        deadline_tick: u64::MAX,
        idempotency_key: Some(
            IdempotencyKey::new("cancel-active", InvocationLimits::default()).unwrap(),
        ),
        request: HostRequest::Program(ProgramRequest::Cancel {
            programs: vec![ProgramName::new("MIDDLE", 128).unwrap()],
        }),
    };
    assert_eq!(
        fixture.router.invoke(&parent(), cancel).outcome,
        Err(HostProblem::Unsupported)
    );
    assert_eq!(
        fixture.router.finish_run_unit(&parent()),
        Err(HostProblem::UnknownOutcome)
    );
    assert_eq!(
        fixture
            .call(&parent(), "MIDDLE", 3, call_payload(&[]))
            .outcome,
        Err(HostProblem::UnknownOutcome)
    );
    assert_eq!(
        fixture
            .store
            .list_provider_state("hardening-49-business", 128)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn hardening_47_reply_and_ready_state_cannot_partially_commit() {
    let root = TestRoot::new();
    let store = Arc::new(MemoryStore::new(StoreLimits {
        max_blob_bytes: 1024,
        ..Default::default()
    }));
    let fixture = Fixture::new(&root, store, HostProblem::NotFound, false);
    let source = INSTANCE_COUNTER.replace(
        "01 N PIC 9 VALUE 0.",
        "01 N PIC 9 VALUE 0.\n01 BIG-STATE PIC X(2048) VALUE SPACES.",
    );
    fixture.install("COUNTER", &source);
    // Reservation fits, retained state cannot fit. The atomic ready+reply write
    // must publish neither. A retry must not reconstruct a fresh ordinary N.
    assert_eq!(
        fixture
            .call(&parent(), "COUNTER", 1, call_payload(&[vec![b'0']]))
            .outcome,
        Err(HostProblem::UnknownOutcome)
    );
    let receipts = fixture
        .store
        .list_provider_state("cobol-call-replay@1", 128)
        .unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].version, 1);
    assert_eq!(
        fixture
            .call(&parent(), "COUNTER", 1, call_payload(&[vec![b'0']]))
            .outcome,
        Err(HostProblem::UnknownOutcome)
    );
    assert_eq!(
        fixture
            .call(&parent(), "COUNTER", 2, call_payload(&[vec![b'0']]))
            .outcome,
        Err(HostProblem::UnknownOutcome)
    );
    assert_eq!(
        fixture.router.finish_run_unit(&parent()),
        Err(HostProblem::UnknownOutcome)
    );
}

#[test]
fn hardening_47_artifact_replacement_requires_cancel_and_never_reinterprets_old_state() {
    let root = TestRoot::new();
    let fixture = Fixture::new(
        &root,
        Arc::new(MemoryStore::new(Default::default())),
        HostProblem::NotFound,
        false,
    );
    fixture.install("COUNTER", INSTANCE_COUNTER);
    fixture.install(
        "COUNTER-NEW",
        &INSTANCE_COUNTER.replace("ADD 1 TO N", "ADD 2 TO N"),
    );
    counter_call(&fixture, &parent(), 1, b'1');
    let new = fixture
        .store
        .get_provider_state("batch-program", "COUNTER-NEW")
        .unwrap()
        .unwrap();
    fixture
        .store
        .put_provider_state(
            ProviderStateRecord {
                namespace: "batch-program".into(),
                key: "COUNTER".into(),
                version: 2,
                payload: new.payload,
            },
            Some(1),
        )
        .unwrap();
    assert_eq!(
        fixture
            .call(&parent(), "COUNTER", 2, call_payload(&[vec![b'0']]))
            .outcome,
        Err(HostProblem::IdempotencyConflict)
    );
    let effect = EffectRequest {
        run_unit: parent().run_unit_id,
        sequence: 3,
        deadline_tick: u64::MAX,
        idempotency_key: Some(
            IdempotencyKey::new("cancel-replaced", InvocationLimits::default()).unwrap(),
        ),
        request: HostRequest::Program(ProgramRequest::Cancel {
            programs: vec![ProgramName::new("COUNTER", 128).unwrap()],
        }),
    };
    assert!(fixture.router.invoke(&parent(), effect).outcome.is_ok());
    counter_call(&fixture, &parent(), 4, b'2');
}

#[test]
fn hardening_47_counter_era_and_replay_only_runs_require_drain_before_upgrade() {
    let root = TestRoot::new();
    let fixture = Fixture::new(
        &root,
        Arc::new(MemoryStore::new(Default::default())),
        HostProblem::NotFound,
        false,
    );
    fixture.install("COUNTER", INSTANCE_COUNTER);
    let key = super::replay::digest(&[b"run-protocol", parent().run_unit_id.as_str().as_bytes()]);
    fixture
        .store
        .put_provider_state(
            ProviderStateRecord {
                namespace: "cobol-call-protocol@1".into(),
                key,
                version: 1,
                payload: b"installed-call@1".to_vec(),
            },
            None,
        )
        .unwrap();
    assert_eq!(
        fixture
            .call(&parent(), "COUNTER", 1, call_payload(&[vec![b'0']]))
            .outcome,
        Err(HostProblem::UnknownOutcome)
    );
    assert!(
        fixture
            .store
            .list_provider_state("cobol-call-replay@1", 128)
            .unwrap()
            .is_empty()
    );
}
