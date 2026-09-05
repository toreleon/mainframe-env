//! Installed-program regressions. The fake effect provider commits real store
//! state; compiler, installation, host dispatch, interpreter and journal are real.
use super::*;
use mainframe_env_batch::DdPlan;
use mainframe_env_execution_api::{CapabilityId, PrincipalId, ResourceLimits, ServiceClass};
use mainframe_env_host_api::{HostLimits, ProgramName, RegistrySnapshot};
use mainframe_env_store::{MemoryStore, SqliteStateStore, StoreLimits};
use mainframe_env_store_api::{ArtifactRecord, EffectState};
use sha2::{Digest, Sha256};
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
        router
            .bind_runtime(host.clone(), store.clone(), &root.0)
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
        let payload = artifact.payload().to_vec();
        let digest: [u8; 32] = Sha256::digest(&payload).into();
        let id = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(&payload)),
            InvocationLimits::default(),
        )
        .unwrap();
        LocalArtifactStore::open(&self.root, 64 * 1024 * 1024)
            .unwrap()
            .put_artifact(ArtifactRecord {
                artifact: id.clone(),
                media_type: "application/vnd.mainframe-env.core-mir".into(),
                payload_digest: digest,
                payload,
            })
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
            .effect
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
        if profile != "call" {
            assert_eq!(
                receipts.len(),
                1,
                "outer durable call retains a reconciliation receipt"
            );
            assert_eq!(receipts[0].state, EffectState::UnknownOutcome);
            assert!(receipts[0].result_digest.is_some());
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
                .reconcile_unknown(&receipt.key, EffectState::Completed, [49; 32])
                .unwrap();
            assert_eq!(reconciled.key, receipt.key);
            assert_eq!(reconciled.state, EffectState::Completed);
            assert!(reopened.unknown_effects(128).unwrap().is_empty());
        }
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
            fixture.batch("ROOT", "").outcome,
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
            HostRequest::Program(ProgramRequest::Call { program, .. }) if program.as_str() == "MIDDLE")).unwrap();
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
        attempt: 0,
        max_attempts: 3,
        available_tick: 0,
        deadline_tick: u64::MAX,
        cancellation_requested: false,
        worker_id: None,
        lease_id: None,
        lease_expiry_tick: None,
        heartbeat_tick: None,
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
        fixture
            .router
            .cobol
            .execute_installed(&inv, "CPU-LOOP", &call_payload(&[])),
        Err(HostProblem::Cancelled)
    );
}
