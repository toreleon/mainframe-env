use super::*;
use crate::cobol::*;
use mainframe_env_compiler::CobolCompiler;
use mainframe_env_compiler_api::*;
use mainframe_env_execution_api::*;
use mainframe_env_host_api::*;
use mainframe_env_interpreter::{CoordinatorLimits, ExecutionCoordinator};
use mainframe_env_store::{LocalArtifactStore, MemoryStore, SqliteStateStore};
use mainframe_env_store_api::*;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub(super) const SOURCE: &str = "IDENTIFICATION DIVISION. PROGRAM-ID. MQFLOW. DATA DIVISION. WORKING-STORAGE SECTION. 01 QM PIC X(48) VALUE SPACES. 01 HC PIC S9(9) BINARY. 01 OLD-HC PIC S9(9) BINARY. 01 CC PIC S9(9) BINARY. 01 RC PIC S9(9) BINARY. 01 CNO. 02 CID PIC X(4) VALUE 'CNO '. 02 CV PIC S9(9) BINARY VALUE 1. 02 OPT PIC S9(9) BINARY VALUE 0. PROCEDURE DIVISION. CALL 'MQCONN' USING QM HC CC RC. IF CC NOT = 0 OR RC NOT = 0 DISPLAY 'BAD-CONN' END-IF. MOVE HC TO OLD-HC. CALL 'MQCONNX' USING QM CNO HC CC RC. IF CC NOT = 1 OR RC NOT = 2002 OR HC NOT = OLD-HC DISPLAY 'BAD-WARNING' END-IF. CALL 'MQCMIT' USING HC CC RC. IF CC NOT = 0 OR RC NOT = 0 DISPLAY 'BAD-CMIT' END-IF. CALL 'MQBACK' USING HC CC RC. IF CC NOT = 0 OR RC NOT = 0 DISPLAY 'BAD-BACK' END-IF. CALL 'MQDISC' USING HC CC RC. IF CC NOT = 0 OR RC NOT = 0 DISPLAY 'BAD-DISC' END-IF. DISPLAY 'DONE'. GOBACK.";

pub(super) struct Clock(pub AtomicU64, pub Mutex<Option<Box<dyn FnMut() + Send>>>);
impl MqReplayClock for Clock {
    fn now_tick(&self) -> Result<u64, HostProblem> {
        if let Some(hook) = self.1.lock().unwrap().as_mut() {
            hook();
        }
        Ok(self.0.load(Ordering::SeqCst))
    }
}
#[derive(Default)]
pub(super) struct Saf {
    pub deny: AtomicBool,
    pub resources: Mutex<Vec<EnterpriseResource>>,
    pub hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl EnterpriseAuthorizer for Saf {
    fn authorize(&self, _: &PrincipalId, resource: &EnterpriseResource) -> Result<(), HostProblem> {
        self.resources.lock().unwrap().push(resource.clone());
        if let Some(hook) = self.hook.lock().unwrap().take() {
            hook();
        }
        if self.deny.load(Ordering::SeqCst) {
            Err(HostProblem::Unauthorized)
        } else {
            Ok(())
        }
    }
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SetupRow {
    namespace: String,
    key: String,
    version: u64,
    payload: Vec<u8>,
}
pub(super) fn normalize_fixture(store: &dyn PlatformStore) {
    // Reproducible provider-generated setup data only. Not production import.
    let stages: Vec<Vec<SetupRow>> =
        serde_json::from_slice(include_bytes!("rich_fixture.json")).unwrap();
    for rows in stages {
        for prior in store.list_provider_state_prefix("mq-", 4096).unwrap() {
            if !rows
                .iter()
                .any(|r| r.namespace == prior.namespace && r.key == prior.key)
            {
                store
                    .delete_provider_state(&prior.namespace, &prior.key, prior.version)
                    .unwrap();
            }
        }
        for row in rows {
            let prior = store.get_provider_state(&row.namespace, &row.key).unwrap();
            let next = ProviderStateRecord {
                namespace: row.namespace,
                key: row.key,
                version: row.version,
                payload: row.payload,
            };
            if prior.as_ref() != Some(&next) {
                store
                    .put_provider_state(next, prior.map(|r| r.version))
                    .unwrap();
            }
        }
    }
}
pub(super) fn legacy_fixture(store: &dyn PlatformStore) {
    let stages: Vec<Vec<SetupRow>> =
        serde_json::from_slice(include_bytes!("rich_fixture.json")).unwrap();
    for row in &stages[1] {
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: row.namespace.clone(),
                    key: row.key.clone(),
                    version: row.version,
                    payload: row.payload.clone(),
                },
                None,
            )
            .unwrap();
    }
}
pub(super) fn descriptor() -> CapabilityDescriptor {
    CapabilityDescriptor {
        capability: CapabilityId::new("host.mq.write", Default::default()).unwrap(),
        provider_id: "mainframe-env-mq".into(),
        generation: "configured-test".into(),
        request_schema: "mainframe-env.host-request@1".into(),
        result_schema: "mainframe-env.host-result@1".into(),
        max_request_bytes: 8 << 20,
        max_result_bytes: 8 << 20,
        ready: true,
    }
}
struct WeakRouter {
    router: Weak<DefaultProgramRouter>,
    descriptor: CapabilityDescriptor,
}
impl HostProvider for WeakRouter {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }
    fn invoke(&self, original: &Invocation, effect: EffectRequest) -> EffectResult {
        self.router.upgrade().unwrap().invoke(original, effect)
    }
}
pub(super) struct RecordingFactory {
    pub host: Arc<ConfiguredInstalledMqHost>,
    pub children: Mutex<Vec<Invocation>>,
    pub observations: Mutex<Vec<Arc<dyn mainframe_env_interpreter::MqMqiProgramFrame>>>,
    pub hook: Mutex<Option<Box<dyn FnOnce(&InstalledBatchAdmission<'_>) + Send>>>,
    pub nested_hook: Mutex<Option<Box<dyn FnOnce(&InstalledBatchAdmission<'_>) + Send>>>,
    pub override_host: Mutex<Option<Arc<ConfiguredInstalledMqHost>>>,
    pub fail_preparation: AtomicBool,
    pub drop_at_admission: AtomicBool,
}
impl ProgramMqHostAdmission for RecordingFactory {
    fn admit_installed_batch(
        &self,
        proof: &InstalledBatchAdmission<'_>,
    ) -> Result<Box<dyn InstalledMqFrameSession>, HostProblem> {
        if let Some(hook) = self.hook.lock().unwrap().take() {
            hook(proof);
        }
        if proof.parent().parent_execution_id.is_some() {
            let hook = self.nested_hook.lock().unwrap().take();
            if let Some(hook) = hook {
                hook(proof);
            }
        }
        let selected = self
            .override_host
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| self.host.clone());
        let session = selected.admit_installed_batch(proof)?;
        self.children.lock().unwrap().push(proof.child().clone());
        self.observations
            .lock()
            .unwrap()
            .push(session.program_frame()?);
        if self.drop_at_admission.load(Ordering::SeqCst) {
            drop(session);
            return Err(HostProblem::UnknownOutcome);
        }
        if self.fail_preparation.load(Ordering::SeqCst) {
            Ok(Box::new(FailedPreparation(session)))
        } else {
            Ok(session)
        }
    }
}
pub(super) struct Fixture {
    pub root: hardening::TestRoot,
    pub store: Arc<dyn PlatformStore>,
    pub host: Arc<ScopedHostService>,
    pub mq: Arc<ConfiguredInstalledMqHost>,
    pub factory: Arc<RecordingFactory>,
    pub router: Arc<DefaultProgramRouter>,
    pub artifacts: Arc<LocalArtifactStore>,
    pub parent: Invocation,
    pub clock: Arc<Clock>,
    pub saf: Arc<Saf>,
    pub url: String,
}
impl Fixture {
    pub fn new(sqlite: bool) -> Self {
        Self::bounded(sqlite, 16)
    }
    pub fn bounded(sqlite: bool, frames: usize) -> Self {
        Self::registered(sqlite, frames, false, false)
    }
    pub fn registered(sqlite: bool, frames: usize, wrapped: bool, foreign_control: bool) -> Self {
        Self::quota(sqlite, frames, wrapped, foreign_control, 65536)
    }
    pub fn quota(
        sqlite: bool,
        frames: usize,
        wrapped: bool,
        foreign_control: bool,
        capacity: usize,
    ) -> Self {
        let root = hardening::TestRoot::new();
        let url = format!(
            "sqlite://{}?mode=rwc",
            root.0.join("configured.db").display()
        );
        let store: Arc<dyn PlatformStore> = if sqlite {
            Arc::new(SqliteStateStore::open(&url, 64 << 20, capacity).unwrap())
        } else {
            Arc::new(MemoryStore::new(mainframe_env_store::StoreLimits {
                max_audits: capacity,
                ..Default::default()
            }))
        };
        normalize_fixture(&*store);
        Self::from_normalized(root, store, url, frames, wrapped, foreign_control)
    }
    pub fn same_store(store: Arc<dyn PlatformStore>, url: String) -> Self {
        Self::from_normalized(hardening::TestRoot::new(), store, url, 16, false, false)
    }
    fn from_normalized(
        root: hardening::TestRoot,
        store: Arc<dyn PlatformStore>,
        url: String,
        frames: usize,
        wrapped: bool,
        foreign_control: bool,
    ) -> Self {
        let saf = Arc::new(Saf::default());
        let clock = Arc::new(Clock(AtomicU64::new(20), Mutex::new(None)));
        let mq = ConfiguredInstalledMqHost::open(
            store.clone(),
            saf.clone(),
            clock.clone(),
            descriptor(),
            Default::default(),
            Default::default(),
            Default::default(),
            3,
            5,
            InstalledMqHostBounds {
                max_roots: 8,
                max_frames: frames,
            },
        )
        .unwrap();
        let router = default_program_router();
        let factory = Arc::new(RecordingFactory {
            host: mq.clone(),
            children: Mutex::new(vec![]),
            observations: Mutex::new(vec![]),
            hook: Mutex::new(None),
            nested_hook: Mutex::new(None),
            override_host: Mutex::new(None),
            fail_preparation: AtomicBool::new(false),
            drop_at_admission: AtomicBool::new(false),
        });
        let control = if foreign_control {
            Arc::new(ClockControl(clock.clone())) as Arc<dyn ProgramExecutionControl>
        } else {
            mq.execution_control()
        };
        router.bind_execution_control(control).unwrap();
        router.bind_mqi_program_host(factory.clone()).unwrap();
        let selected: Arc<dyn HostProvider> = if wrapped {
            Arc::new(Forwarder(mq.clone()))
        } else {
            mq.clone()
        };
        let host = Arc::new(ScopedHostService::new(
            Arc::new(
                RegistrySnapshot::new(
                    1,
                    vec![
                        selected,
                        Arc::new(WeakRouter {
                            router: Arc::downgrade(&router),
                            descriptor: router.descriptor().clone(),
                        }),
                    ],
                    Default::default(),
                )
                .unwrap(),
            ),
            Default::default(),
        ));
        let artifacts = Arc::new(LocalArtifactStore::open(&root.0, 64 << 20).unwrap());
        router
            .bind_runtime(host.clone(), store.clone(), artifacts.clone())
            .unwrap();
        let mut parent = hardening::parent();
        parent.deadline_tick = 1000;
        parent.cancellation_probe = Some(CancellationProbe::new());
        let mut grants = parent.principal.grants().clone();
        grants.insert(CapabilityId::new("host.mq.write", Default::default()).unwrap());
        parent.principal =
            Principal::new(parent.principal.id().clone(), grants, Default::default()).unwrap();
        Self {
            root,
            store,
            host,
            mq,
            factory,
            router,
            artifacts,
            parent,
            clock,
            saf,
            url,
        }
    }
    pub fn install(&self, name: &str, source: &str) {
        let CompilerResult::Published { artifact, .. } = CobolCompiler::default()
            .compile(CompilerRequest {
                source: source_bundle(&hardening::input(source)).unwrap(),
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").unwrap(),
                options: CompileOptions::new(BTreeMap::new()).unwrap(),
            })
            .unwrap()
        else {
            panic!("actual compiled artifact")
        };
        let record = artifact::published_artifact_record(&artifact).unwrap();
        let id = record.artifact.clone();
        self.artifacts.put_artifact(record).unwrap();
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
    pub fn batch_effect(&self, sequence: u64, name: &str) -> EffectRequest {
        EffectRequest {
            run_unit: self.parent.run_unit_id.clone(),
            sequence,
            deadline_tick: self.parent.deadline_tick,
            idempotency_key: Some(
                IdempotencyKey::new(format!("original-call-{sequence}"), Default::default())
                    .unwrap(),
            ),
            request: HostRequest::Program(ProgramRequest::Call {
                program: ProgramName::new(name, 128).unwrap(),
                payload: BoundedPayload::new(
                    "mainframe-env.program.input@1",
                    serde_json::to_vec(&hardening::input("")).unwrap(),
                    Default::default(),
                )
                .unwrap(),
                service: None,
            }),
        }
    }
    pub fn effect(&self, sequence: u64, name: &str) -> EffectRequest {
        self.call_effect(sequence, name, &[])
    }
    pub fn call_effect(&self, sequence: u64, name: &str, arguments: &[Vec<u8>]) -> EffectRequest {
        let mut effect = self.batch_effect(sequence, name);
        let HostRequest::Program(ProgramRequest::Call { payload, .. }) = &mut effect.request else {
            unreachable!()
        };
        *payload = hardening::call_payload(arguments);
        effect
    }
    pub fn run(&self, effect: EffectRequest) -> (ExecutionOutcome, EffectResult) {
        let (outcome, reply) = self.run_raw(effect);
        (outcome, reply.unwrap())
    }
    pub fn run_raw(&self, effect: EffectRequest) -> (ExecutionOutcome, Option<EffectResult>) {
        let mut caller = Caller {
            effect: Some(effect),
            reply: None,
        };
        let outcome = ExecutionCoordinator::durable(
            self.host.clone(),
            self.store.clone(),
            CoordinatorLimits::default(),
        )
        .execute_with_control(&mut caller, &self.parent, || {
            self.mq.control.observe(&self.parent)
        });
        (outcome, caller.reply)
    }
    pub fn rows(&self) -> Vec<ProviderStateRecord> {
        self.store.list_provider_state_prefix("mq-", 4096).unwrap()
    }
    pub fn run_calls(&self, effects: Vec<EffectRequest>) -> (ExecutionOutcome, Vec<EffectResult>) {
        let mut caller = Calls {
            effects: effects.into_iter(),
            replies: vec![],
        };
        let outcome = ExecutionCoordinator::durable(
            self.host.clone(),
            self.store.clone(),
            Default::default(),
        )
        .execute_with_control(&mut caller, &self.parent, || {
            self.mq.control.observe(&self.parent)
        });
        (outcome, caller.replies)
    }
}
// Deliberately wrong physical registration. It delegates to the REAL provider;
// equal descriptor and same inner service must still fail Arc selection proof.
struct Forwarder(Arc<ConfiguredInstalledMqHost>);
impl HostProvider for Forwarder {
    fn descriptor(&self) -> &CapabilityDescriptor {
        self.0.descriptor()
    }
    fn invoke(&self, original: &Invocation, effect: EffectRequest) -> EffectResult {
        self.0.invoke(original, effect)
    }
}
struct FailedPreparation(Box<dyn InstalledMqFrameSession>);
impl InstalledMqFrameSession for FailedPreparation {
    fn store(&self) -> &Arc<dyn PlatformStore> {
        self.0.store()
    }
    fn execution_control(&self) -> &Arc<dyn ProgramExecutionControl> {
        self.0.execution_control()
    }
    fn program_frame(
        &self,
    ) -> Result<Arc<dyn mainframe_env_interpreter::MqMqiProgramFrame>, HostProblem> {
        Err(HostProblem::Malformed)
    }
    fn abort_preparation(&mut self, problem: &HostProblem) -> Result<(), HostProblem> {
        self.0.abort_preparation(problem)
    }
    fn finish(&mut self, outcome: &ExecutionOutcome) -> Result<(), HostProblem> {
        self.0.finish(outcome)
    }
}
struct Calls {
    effects: std::vec::IntoIter<EffectRequest>,
    replies: Vec<EffectResult>,
}
impl Machine for Calls {
    type Effect = EffectRequest;
    type EffectResult = EffectResult;
    fn drive(
        &mut self,
        resume: MachineResume<EffectResult>,
        _: Quantum,
    ) -> MachineDrive<EffectRequest> {
        if let MachineResume::HostResult(reply) = resume {
            self.replies.push(reply);
        }
        if let Some(effect) = self.effects.next() {
            MachineDrive::HostCall(effect)
        } else {
            MachineDrive::Completed(Completion {
                return_code: 0,
                output: BoundedPayload::new("test@1", vec![], Default::default()).unwrap(),
            })
        }
    }
}
struct Caller {
    effect: Option<EffectRequest>,
    reply: Option<EffectResult>,
}
impl Machine for Caller {
    type Effect = EffectRequest;
    type EffectResult = EffectResult;
    fn drive(
        &mut self,
        resume: MachineResume<EffectResult>,
        _: Quantum,
    ) -> MachineDrive<EffectRequest> {
        match resume {
            MachineResume::Start => MachineDrive::HostCall(self.effect.take().unwrap()),
            MachineResume::HostResult(reply) => {
                let output = match &reply.outcome {
                    Ok(HostResult::Program(p)) => p.clone(),
                    _ => BoundedPayload::new("test@1", vec![], Default::default()).unwrap(),
                };
                self.reply = Some(reply);
                MachineDrive::Completed(Completion {
                    return_code: 0,
                    output,
                })
            }
            _ => panic!("caller continuation"),
        }
    }
}
