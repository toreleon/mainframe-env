//! Actual compiler/artifact/program/machine/core route with a fixture MQ provider.
//! This proves the handoff and original journal identity, not MQ queue/SAF acceptance.

use super::*;
use mainframe_env_execution_api::{CapabilityId, Principal};
use mainframe_env_host_api::mq_mqi::{
    MqMqiOutcome, MqMqiOutput, MqMqiRequest, MqMqiResult, MqMqiStatus,
};
use mainframe_env_host_api::{
    MqHandleRegistry, MqHandleSharing, MqMqiHostResult, RegistrySnapshot, canonical_request_digest,
};
use mainframe_env_store::{LocalArtifactStore, SqliteStateStore};
use mainframe_env_store_api::{ArtifactStore, EffectState, IdempotencyStore};

struct Provider {
    descriptor: CapabilityDescriptor,
    store: Arc<dyn PlatformStore>,
    registry: Mutex<MqHandleRegistry>,
    keys: Mutex<Vec<IdempotencyKey>>,
}
impl HostProvider for Provider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }
    fn invoke(&self, invocation: &Invocation, effect: EffectRequest) -> EffectResult {
        let occurrence = effect
            .mq_mqi_occurrence(Default::default())
            .unwrap()
            .unwrap();
        let key = effect.idempotency_key.as_ref().unwrap();
        let intent = self.store.effect(key).unwrap().unwrap();
        assert_eq!(intent.state, EffectState::Intent);
        assert_eq!(intent.execution_id, invocation.execution_id);
        assert_eq!(
            intent.request_digest,
            canonical_request_digest(&effect.request).unwrap()
        );
        self.keys.lock().unwrap().push(key.clone());
        let envelope = occurrence.envelope();
        let output = match envelope.request {
            MqMqiRequest::Connect(_) => MqMqiOutput::Connected(
                self.registry
                    .lock()
                    .unwrap()
                    .connect(envelope.context.owner, MqHandleSharing::NonShared)
                    .unwrap(),
            ),
            MqMqiRequest::Disconnect { connection } => {
                self.registry
                    .lock()
                    .unwrap()
                    .disconnect(envelope.context.owner, connection)
                    .unwrap();
                MqMqiOutput::NoOutput
            }
            _ => panic!("fixture handles only the tested connection flow"),
        };
        EffectResult {
            sequence: effect.sequence,
            outcome: Ok(HostResult::MqMqi(MqMqiHostResult {
                limits: envelope.limits,
                result: MqMqiResult {
                    call: envelope.request.call(),
                    outcome: MqMqiOutcome::Completed {
                        status: MqMqiStatus::OkNone,
                        output,
                    },
                },
            })),
        }
    }
}

#[test]
fn compiled_installed_batch_emits_original_typed_effects_and_real_durable_intents() {
    for sqlite in [false, true] {
        let root = super::super::super::hardening::TestRoot::new();
        let url = format!("sqlite://{}?mode=rwc", root.0.join("mqi.db").display());
        let store: Arc<dyn PlatformStore> = if sqlite {
            Arc::new(SqliteStateStore::open(&url, 8 * 1024 * 1024, 65536).unwrap())
        } else {
            Arc::new(MemoryStore::new(Default::default()))
        };
        let router = default_program_router();
        let factory = admission(false);
        router.bind_mqi_program_host(factory.clone()).unwrap();
        router
            .bind_execution_control(Arc::new(|_: &Invocation| {
                Ok(ExecutionControl {
                    now_tick: 1,
                    cancellation_requested: false,
                })
            }))
            .unwrap();
        let limits = InvocationLimits::default();
        let provider = Arc::new(Provider {
            descriptor: CapabilityDescriptor {
                capability: CapabilityId::new("host.mq.write", limits).unwrap(),
                provider_id: "mqi-host-fixture".into(),
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
        });
        let host = Arc::new(ScopedHostService::new(
            Arc::new(
                RegistrySnapshot::new(1, vec![provider.clone() as Arc<dyn HostProvider>], limits)
                    .unwrap(),
            ),
            Default::default(),
        ));
        let artifacts = Arc::new(LocalArtifactStore::open(&root.0, 64 * 1024 * 1024).unwrap());
        router
            .bind_runtime(host, store.clone(), artifacts.clone())
            .unwrap();
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. MQFLOW. DATA DIVISION. WORKING-STORAGE SECTION. 01 QM PIC X(48) VALUE SPACES. 01 HC PIC S9(9) BINARY. 01 CC PIC S9(9) BINARY. 01 RC PIC S9(9) BINARY. PROCEDURE DIVISION. CALL 'MQCONN' USING QM HC CC RC. CALL 'MQDISC' USING HC CC RC. DISPLAY 'DONE'. STOP RUN.";
        let input = super::super::super::hardening::input(source);
        let CompilerResult::Published { artifact, .. } = CobolCompiler::default()
            .compile(CompilerRequest {
                source: source_bundle(&input).unwrap(),
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").unwrap(),
                options: CompileOptions::new(BTreeMap::new()).unwrap(),
            })
            .unwrap()
        else {
            panic!("compiled artifact")
        };
        let record = artifact::published_artifact_record(&artifact).unwrap();
        let id = record.artifact.clone();
        artifacts.put_artifact(record).unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "batch-program".into(),
                    key: "MQFLOW".into(),
                    version: 1,
                    payload: id.as_str().as_bytes().to_vec(),
                },
                None,
            )
            .unwrap();
        let admitted = router
            .cobol
            .preflight_installed_program("MQFLOW", true)
            .unwrap();
        let mut parent = super::super::super::hardening::parent();
        parent.deadline_tick = 1000;
        let mut grants = parent.principal.grants().clone();
        grants.insert(CapabilityId::new("host.mq.write", limits).unwrap());
        parent.principal = Principal::new(parent.principal.id().clone(), grants, limits).unwrap();
        let payload = BoundedPayload::new(
            "mainframe-env.program.input@1",
            serde_json::to_vec(&super::super::super::hardening::input("")).unwrap(),
            limits,
        )
        .unwrap();
        let output = router
            .cobol
            .execute_installed_batch(&parent, "MQFLOW", admitted, &payload, "typed-mqi")
            .unwrap();
        assert_eq!(output.records, vec![b"DONE".to_vec()]);
        let keys = provider.keys.lock().unwrap().clone();
        assert_eq!(keys.len(), 2);
        for key in &keys {
            assert_eq!(
                store.effect(key).unwrap().unwrap().state,
                EffectState::Completed
            );
        }
        assert_eq!(provider.registry.lock().unwrap().active_handles(), 0);
        let observed = factory.observed.lock().unwrap();
        assert_eq!(observed.len(), 1);
        assert_eq!(
            observed[0].parent_execution_id.as_ref(),
            Some(&parent.execution_id)
        );
        assert_eq!(observed[0].principal, parent.principal);
        assert_eq!(observed[0].cancellation_probe, parent.cancellation_probe);
        assert_eq!(observed[0].artifact, id);
        if sqlite {
            let reopened = SqliteStateStore::open(&url, 8 * 1024 * 1024, 65536).unwrap();
            for key in &keys {
                assert_eq!(
                    reopened.effect(key).unwrap().unwrap().state,
                    EffectState::Completed
                );
            }
        }
    }
}
