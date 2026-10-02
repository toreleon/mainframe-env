//! Real physical backend fixtures; not installed host attestation/participant evidence.
use super::*;
use crate::{MqObjectName, MqQueueDefinition};
use mainframe_env_execution_api::*;
use mainframe_env_host_api::mq_mqi::*;
use mainframe_env_host_api::mq_object_route::*;
use mainframe_env_host_api::*;
use mainframe_env_store::{MemoryStore, SqliteStateStore, StoreLimits};
use mainframe_env_store_api::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

mod connection_warning;
mod flows;
mod installed_fixture;
mod lifecycle;
mod publication;
mod refusals;
mod replay;

struct Clock(AtomicU64);
impl MqReplayClock for Clock {
    fn now_tick(&self) -> Result<u64, HostProblem> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}
#[derive(Default)]
struct Saf {
    deny: AtomicBool,
    calls: AtomicU64,
    hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl EnterpriseAuthorizer for Saf {
    fn authorize(&self, _: &PrincipalId, _: &EnterpriseResource) -> Result<(), HostProblem> {
        self.calls.fetch_add(1, Ordering::SeqCst);
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
struct Fixture {
    store: Arc<dyn PlatformStore>,
    runtime: MqTrustedBatchRuntime,
    parent: Invocation,
    clock: Arc<Clock>,
    saf: Arc<Saf>,
}
fn backend(sqlite: bool) -> Arc<dyn PlatformStore> {
    if sqlite {
        Arc::new(SqliteStateStore::open("sqlite::memory:", 64 << 20, 256).unwrap())
    } else {
        Arc::new(MemoryStore::new(StoreLimits {
            max_audits: 256,
            ..Default::default()
        }))
    }
}
fn descriptor() -> CapabilityDescriptor {
    CapabilityDescriptor {
        capability: CapabilityId::new("host.mq.write", Default::default()).unwrap(),
        provider_id: "mainframe-env-mq".into(),
        generation: "selected-test".into(),
        request_schema: "mainframe-env.mq-request@1".into(),
        result_schema: "mainframe-env.mq-result@1".into(),
        max_request_bytes: 4 << 20,
        max_result_bytes: 4 << 20,
        ready: true,
    }
}
fn open(
    store: Arc<dyn PlatformStore>,
    saf: Arc<Saf>,
    clock: Arc<Clock>,
) -> Result<MqTrustedBatchRuntime, HostProblem> {
    MqTrustedBatchRuntime::open(
        store,
        Default::default(),
        3,
        5,
        saf,
        clock,
        descriptor(),
        Default::default(),
        Default::default(),
    )
}
impl Fixture {
    fn new(sqlite: bool) -> Self {
        Self::from_store(backend(sqlite))
    }
    fn from_store(store: Arc<dyn PlatformStore>) -> Self {
        // Explicit TEST preparation through the existing bounded private plan;
        // the production facet never initializes/normalizes/imports any state.
        let legacy = MqService::open(store.clone(), Default::default()).unwrap();
        legacy
            .install(vec![MqQueueDefinition {
                name: "Q".into(),
                trigger_program: None,
            }])
            .unwrap();
        let plan = legacy
            .plan_legacy_delivery_import(3, 5, Default::default())
            .unwrap();
        store
            .mutate_provider_states_atomic(plan.into_parts().0)
            .unwrap();
        drop(legacy);
        let l = InvocationLimits::default();
        let parent = Invocation::new(
            RequestId::new("request", l).unwrap(),
            ExecutionId::new("parent", l).unwrap(),
            RunUnitId::new("run", l).unwrap(),
            None,
            Selector::new("test", l).unwrap(),
            ArtifactRef::new("artifact", l).unwrap(),
            Principal::new(
                PrincipalId::new("TEST", l).unwrap(),
                BTreeSet::from([
                    CapabilityId::new("host.mq.write", l).unwrap(),
                    CapabilityId::new("host.program.invoke", l).unwrap(),
                ]),
                l,
            )
            .unwrap(),
            ServiceClass::System,
            0,
            1000,
            TraceId::new("trace", l).unwrap(),
            IdempotencyKey::new("parent-invocation", l).unwrap(),
            1,
            ResourceLimits::default(),
            BTreeMap::new(),
            l,
        )
        .unwrap()
        .with_cancellation_probe(CancellationProbe::new());
        let clock = Arc::new(Clock(AtomicU64::new(20)));
        let saf = Arc::new(Saf::default());
        let runtime = open(store.clone(), saf.clone(), clock.clone()).unwrap();
        seed_execution(&*store, &parent);
        Self {
            store,
            runtime,
            parent,
            clock,
            saf,
        }
    }
    fn root(&self) -> MqTrustedBatchRoot {
        self.runtime.admit_root(self.parent.clone()).unwrap()
    }
    fn child(&self, parent: &MqTrustedBatchFrame, name: &str) -> MqTrustedBatchFrame {
        let inv = child_invocation(parent.original(), name);
        seed_execution(&*self.store, &inv);
        self.runtime
            .prepare_same_task_child(parent, inv, MqTrustedBatchRelationship::SameTaskCall)
            .unwrap()
    }
    fn rows(&self) -> Vec<ProviderStateRecord> {
        self.store.list_provider_state_prefix("mq-", 4096).unwrap()
    }
    fn call(
        &self,
        frame: &mut MqTrustedBatchFrame,
        sequence: u64,
        request: MqMqiRequest,
    ) -> EffectResult {
        let e = effect(frame, sequence, request);
        seed(&*self.store, frame.original(), &e);
        let result = dispatch(frame, &e).unwrap();
        let rows = self.rows();
        assert_eq!(dispatch(frame, &e).unwrap(), result);
        assert_eq!(self.rows(), rows);
        assert_eq!(
            self.store
                .effect(e.idempotency_key.as_ref().unwrap())
                .unwrap()
                .unwrap()
                .state,
            EffectState::Intent
        );
        result
    }
    fn depth(&self) -> usize {
        self.runtime
            .inner
            .service
            .trusted_batch_test_depth(&MqObjectName::new("Q").unwrap())
    }
}
fn child_invocation(parent: &Invocation, name: &str) -> Invocation {
    let mut inv = parent.clone();
    inv.execution_id = ExecutionId::new(name, Default::default()).unwrap();
    inv.request_id = RequestId::new(format!("{name}-request"), Default::default()).unwrap();
    inv.idempotency_key =
        IdempotencyKey::new(format!("{name}-invocation"), Default::default()).unwrap();
    inv.parent_execution_id = Some(parent.execution_id.clone());
    inv
}
fn seed_execution(store: &dyn PlatformStore, inv: &Invocation) {
    store
        .create_execution(ExecutionRecord {
            execution_id: inv.execution_id.clone(),
            run_unit_id: inv.run_unit_id.clone(),
            selector: inv.selector.clone(),
            artifact: inv.artifact.clone(),
            principal: inv.principal.id().clone(),
            state: ExecutionState::Admitted,
            attempt: inv.attempt,
            version: 1,
            owner_lease: None,
            lease_expiry_tick: None,
            terminal_tick: None,
        })
        .unwrap();
    store
        .transition_execution(&inv.execution_id, 1, ExecutionState::Queued, 6)
        .unwrap();
    store
        .transition_execution(&inv.execution_id, 2, ExecutionState::Running, 7)
        .unwrap();
}
fn effect(frame: &MqTrustedBatchFrame, sequence: u64, request: MqMqiRequest) -> EffectRequest {
    let key = IdempotencyKey::new(
        format!("{}-{sequence}", frame.original().execution_id.as_str()),
        Default::default(),
    )
    .unwrap();
    EffectRequest {
        run_unit: frame.original().run_unit_id.clone(),
        sequence,
        deadline_tick: 900,
        idempotency_key: Some(key.clone()),
        request: HostRequest::MqMqi(MqMqiHostRequest {
            mutation: Mutation {
                sequence,
                idempotency_key: key,
                transaction: None,
            },
            envelope: MqMqiRequestEnvelope {
                context: frame.context().unwrap(),
                limits: frame.limits(),
                request,
            },
        }),
    }
}
fn seed(store: &dyn PlatformStore, inv: &Invocation, e: &EffectRequest) {
    store
        .record_intent(EffectRecord {
            execution_id: inv.execution_id.clone(),
            run_unit_id: inv.run_unit_id.clone(),
            sequence: e.sequence,
            key: e.idempotency_key.clone().unwrap(),
            digest_format: EffectDigestFormat::CanonicalHostV1,
            request_digest: canonical_request_digest(&e.request).unwrap(),
            state: EffectState::Intent,
            result_digest: None,
            resolved_tick: None,
            intent: EffectIntentMetadata {
                owner: inv.execution_id.clone(),
                attempt: inv.attempt,
                capability: Some(e.request.required_capability(Default::default())),
                audit_resource: Some(canonical_audit_resource_digest(&e.request)),
                audit_invocation_key: Some(inv.idempotency_key.clone()),
                created_tick: 8,
                recovery_after_tick: 900,
                epoch: 8,
                recovery_lease: None,
            },
        })
        .unwrap();
}
fn dispatch(
    frame: &mut MqTrustedBatchFrame,
    e: &EffectRequest,
) -> Result<EffectResult, HostProblem> {
    frame.dispatch(
        e.mq_mqi_occurrence(HostLimits::default())?
            .ok_or(HostProblem::Malformed)?,
    )
}
fn output(result: EffectResult) -> MqMqiOutput {
    let HostResult::MqMqi(r) = result.outcome.unwrap() else {
        panic!("typed result")
    };
    match r.result.outcome {
        MqMqiOutcome::Completed { output, .. }
        | MqMqiOutcome::ReviewedOutput { output, .. }
        | MqMqiOutcome::StatusPending { output } => output,
        _ => panic!("typed output"),
    }
}
fn connect() -> MqMqiRequest {
    MqMqiRequest::Connect(MqMqiConnect {
        manager: None,
        sharing: MqHandleSharing::NonShared,
        options: MqMqiOptions::ContractDefault,
    })
}
fn lookup() -> MqRouteLookup {
    MqRouteLookup::Queue {
        name: MqRouteName::new("Q").unwrap(),
        manager: None,
        dynamic_pattern: None,
    }
}
fn object_open(c: MqHconn) -> MqMqiRequest {
    MqMqiRequest::Open(
        MqObjectOpenRequest::new(
            c,
            lookup(),
            &[MqRouteOpenAccess::InputShared, MqRouteOpenAccess::Output],
            Default::default(),
        )
        .unwrap(),
    )
}
fn put_request(c: MqHconn, object: MqHobj, unit: MqMqiUnitOfWork) -> MqMqiRequest {
    MqMqiRequest::Put {
        connection: c,
        object,
        put: MqMqiPut {
            message: MqMessage {
                descriptor: MqMessageDescriptor {
                    identifiers: MqMessageIdentifiers {
                        message_id: Some(vec![7; 24]),
                        ..Default::default()
                    },
                    format: Some("BYTES".into()),
                    expiry: MqExpiry::Unlimited,
                    persistence: MqPersistence::Persistent,
                    priority: MqPriority::QueueDefault,
                    ordering: Default::default(),
                },
                body: vec![0, 255, 41],
                properties: vec![],
            },
            message_handle: None,
            context: MqMqiMessageContext::Default,
            options: MqMqiOptions::ContractDefault,
            unit,
        },
    }
}
fn connected(f: &Fixture, frame: &mut MqTrustedBatchFrame) -> (MqHconn, MqHobj) {
    let MqMqiOutput::Connected(c) = output(f.call(frame, 1, connect())) else {
        panic!()
    };
    let MqMqiOutput::Opened { object, .. } = output(f.call(frame, 2, object_open(c))) else {
        panic!()
    };
    (c, object)
}
fn unit(frame: &MqTrustedBatchFrame, c: MqHconn) -> u64 {
    let MqMqiUnitOfWork::Local { unit } = frame.current_unit(c).unwrap() else {
        panic!()
    };
    unit
}
