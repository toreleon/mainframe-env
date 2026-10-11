//! Actual CICS command receipts and conservative readonly cursor replay fences.
use super::*;
use mainframe_env_execution_api::AuditRecord;
use mainframe_env_host_api::{
    DatasetAttributes, DatasetOrganization, DatasetResult, HostLimits, RecordFormat,
    RegistrySnapshot, SecurityDecision,
};
use mainframe_env_store::MemoryStore;
use mainframe_env_store_api::{AuditSink, ProviderStateMutation, ProviderStateWrite};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

struct Clock;
impl CicsReplayClock for Clock {
    fn now_tick(&self) -> Result<u64, HostProblem> {
        Ok(300)
    }
}

struct Store {
    inner: MemoryStore,
    pause: AtomicBool,
    fail_write: AtomicBool,
    conflict_write: AtomicBool,
    entered: Sender<()>,
    release: Mutex<Receiver<()>>,
}
impl AuditSink for Store {
    fn record_audit(&self, row: AuditRecord) -> Result<(), StoreError> {
        self.inner.record_audit(row)
    }
    fn audit_records(
        &self,
        execution: &ExecutionId,
        start: u64,
        max: usize,
    ) -> Result<Vec<AuditRecord>, StoreError> {
        self.inner.audit_records(execution, start, max)
    }
}
impl ProviderStateStore for Store {
    fn advance_logical_clock(&self, floor: u64) -> Result<u64, StoreError> {
        self.inner.advance_logical_clock(floor)
    }
    fn get_provider_state(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<ProviderStateRecord>, StoreError> {
        let row = self.inner.get_provider_state(namespace, key)?;
        if namespace == "cics-effect-replay-v1"
            && key == "original:2"
            && self.pause.swap(false, Ordering::SeqCst)
        {
            self.entered.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
        }
        Ok(row)
    }
    fn list_provider_state(
        &self,
        namespace: &str,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        self.inner.list_provider_state(namespace, max)
    }
    fn list_provider_state_prefix(
        &self,
        namespace: &str,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        self.inner.list_provider_state_prefix(namespace, max)
    }
    fn put_provider_state(
        &self,
        row: ProviderStateRecord,
        expected: Option<u64>,
    ) -> Result<(), StoreError> {
        if row.namespace == "cics-effect-replay-v1" {
            if self.fail_write.swap(false, Ordering::SeqCst) {
                return Err(StoreError::Infrastructure("readonly-receipt-write".into()));
            }
            if self.conflict_write.swap(false, Ordering::SeqCst) {
                return Err(StoreError::Conflict);
            }
        }
        self.inner.put_provider_state(row, expected)
    }
    fn delete_provider_state(
        &self,
        namespace: &str,
        key: &str,
        expected: u64,
    ) -> Result<(), StoreError> {
        self.inner.delete_provider_state(namespace, key, expected)
    }
    fn move_provider_state(
        &self,
        row: ProviderStateRecord,
        key: &str,
        expected: u64,
    ) -> Result<(), StoreError> {
        self.inner.move_provider_state(row, key, expected)
    }
    fn put_provider_states_atomic(&self, rows: Vec<ProviderStateWrite>) -> Result<(), StoreError> {
        self.inner.put_provider_states_atomic(rows)
    }
    fn mutate_provider_states_atomic(
        &self,
        rows: Vec<ProviderStateMutation>,
    ) -> Result<(), StoreError> {
        self.inner.mutate_provider_states_atomic(rows)
    }
}

struct Port {
    descriptor: CapabilityDescriptor,
    requests: Arc<Mutex<Vec<DatasetRequest>>>,
    reads: Arc<AtomicUsize>,
}
impl HostProvider for Port {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }
    fn invoke(&self, _: &Invocation, effect: EffectRequest) -> EffectResult {
        let outcome = match effect.request {
            HostRequest::Security(_) => Ok(HostResult::Security(SecurityDecision::Allow)),
            HostRequest::Dataset(request) => {
                self.requests.lock().unwrap().push(request.clone());
                let reply = match request {
                    DatasetRequest::Attributes { .. } => DatasetResult::Attributes {
                        attributes: DatasetAttributes {
                            organization: DatasetOrganization::KeySequenced,
                            record_format: RecordFormat::Fixed,
                            logical_record_length: 4,
                            key_offset: Some(0),
                            key_length: Some(2),
                            ccsid: None,
                        },
                        version: 1,
                    },
                    DatasetRequest::StartBrowse { .. } | DatasetRequest::EndBrowse { .. } => {
                        DatasetResult::Browse {
                            cursor: "CURSOR-1".into(),
                            record: None,
                            identity: None,
                            key: None,
                        }
                    }
                    DatasetRequest::ReadNext { .. } => {
                        let index = self.reads.fetch_add(1, Ordering::SeqCst);
                        let (key, record) = if index == 0 {
                            (b"AA".to_vec(), b"AA11".to_vec())
                        } else {
                            (b"BB".to_vec(), b"BB22".to_vec())
                        };
                        DatasetResult::Browse {
                            cursor: "CURSOR-1".into(),
                            record: Some(record),
                            identity: Some(key.clone()),
                            key: Some(key),
                        }
                    }
                    _ => {
                        return EffectResult {
                            sequence: effect.sequence,
                            outcome: Err(HostProblem::Unsupported),
                        };
                    }
                };
                Ok(HostResult::Dataset(reply))
            }
            _ => Err(HostProblem::Unsupported),
        };
        EffectResult {
            sequence: effect.sequence,
            outcome,
        }
    }
}
struct Fixture {
    service: Arc<CicsService>,
    actor: Invocation,
    store: Arc<Store>,
    requests: Arc<Mutex<Vec<DatasetRequest>>>,
    reads: Arc<AtomicUsize>,
    entered: Receiver<()>,
    release: Sender<()>,
}
fn fixture() -> Fixture {
    let (notify, entered) = channel();
    let (release, wait) = channel();
    let store = Arc::new(Store {
        inner: MemoryStore::new(Default::default()),
        pause: AtomicBool::new(false),
        fail_write: AtomicBool::new(false),
        conflict_write: AtomicBool::new(false),
        entered: notify,
        release: Mutex::new(wait),
    });
    let requests: Arc<Mutex<Vec<DatasetRequest>>> = Arc::default();
    let reads = Arc::new(AtomicUsize::new(0));
    let providers = ["host.security.authorize", "host.dataset.read"]
        .into_iter()
        .map(|name| {
            Arc::new(Port {
                descriptor: CapabilityDescriptor {
                    capability: CapabilityId::new(name, InvocationLimits::default()).unwrap(),
                    provider_id: "readonly-journal".into(),
                    generation: "1".into(),
                    request_schema: "request@1".into(),
                    result_schema: "result@1".into(),
                    max_request_bytes: 1024 * 1024,
                    max_result_bytes: 1024 * 1024,
                    ready: true,
                },
                requests: requests.clone(),
                reads: reads.clone(),
            }) as Arc<dyn HostProvider>
        })
        .collect();
    let host = Arc::new(ScopedHostService::new(
        Arc::new(RegistrySnapshot::new(1, providers, InvocationLimits::default()).unwrap()),
        HostLimits::default(),
    ));
    let service = CicsService::open_with_replay_clock(
        host,
        store.clone(),
        CicsLimits::default(),
        Arc::new(Clock),
    )
    .unwrap();
    let actor = crate::service::tests::invocation();
    let session = SessionId::new("read-journal", 128).unwrap();
    service.create_session(&session, 24, 80).unwrap();
    service
        .register_run(actor.clone(), &session, "MENU", "ME01", "S001")
        .unwrap();
    Fixture {
        service,
        actor,
        store,
        requests,
        reads,
        entered,
        release,
    }
}
fn offered(
    actor: &Invocation,
    operation: CicsOperation,
    sequence: u64,
) -> (EffectRequest, CicsRequest) {
    let mut arguments = BTreeMap::from([(
        "DATASET".into(),
        BoundedPayload::new(
            "mainframe-env.cics.literal@1",
            b"TEST.DATA".to_vec(),
            InvocationLimits::default(),
        )
        .unwrap(),
    )]);
    if operation == CicsOperation::StartBrowse {
        arguments.insert(
            "RIDFLD".into(),
            BoundedPayload::new(
                "mainframe-env.cics.literal@1",
                b"AA".to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
    }
    let request = CicsRequest {
        operation,
        arguments,
        condition_policy: CicsConditionPolicy::NoHandle,
        mutation: None,
    };
    let effect = EffectRequest {
        run_unit: actor.run_unit_id.clone(),
        sequence,
        deadline_tick: actor.deadline_tick,
        idempotency_key: Some(
            IdempotencyKey::new(format!("original:{sequence}"), InvocationLimits::default())
                .unwrap(),
        ),
        request: HostRequest::Cics(request.clone()),
    };
    assert_eq!(effect.validate(HostLimits::default()), Ok(()));
    assert!(!request.is_mutating());
    (effect, request)
}
fn invoke(f: &Fixture, operation: CicsOperation, sequence: u64) -> CicsResponse {
    let (effect, request) = offered(&f.actor, operation, sequence);
    f.service.invoke(&effect, request).unwrap()
}

#[test]
fn ordinary_browse_replies_retain_exact_actual_owner_request_and_response() {
    let f = fixture();
    for (operation, sequence) in [
        (CicsOperation::StartBrowse, 1),
        (CicsOperation::ReadNext, 2),
        (CicsOperation::EndBrowse, 3),
    ] {
        let (effect, request) = offered(&f.actor, operation, sequence);
        let response = f.service.invoke(&effect, request.clone()).unwrap();
        let row = f
            .store
            .get_provider_state(
                "cics-effect-replay-v1",
                effect.idempotency_key.as_ref().unwrap().as_str(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(row.payload.get(..8), Some(b"MECER003".as_slice()));
        assert_eq!(row.version, 2);
        let retained = decode_cics_effect_replay(&row.payload, CicsLimits::default()).unwrap();
        assert_eq!(retained.response, response);
        assert_eq!(
            retained.effect_key.as_deref(),
            effect.idempotency_key.as_ref().map(IdempotencyKey::as_str)
        );
        assert_eq!(
            retained.owner_execution.as_deref(),
            Some(f.actor.execution_id.as_str())
        );
        assert_eq!(
            retained.owner_run_unit.as_deref(),
            Some(f.actor.run_unit_id.as_str())
        );
        assert_eq!(retained.sequence, Some(sequence));
        assert_eq!(
            retained.request_digest,
            canonical_request_digest(&effect.request).unwrap()
        );
        assert_eq!(
            retained.result_digest,
            Some(handlers::cics_result_digest(&response).unwrap())
        );
        assert_eq!(
            retained.binding_digest,
            Some(cics_effect_replay_binding_digest(&retained))
        );
        assert!(
            retained
                .resolution_tick
                .is_some_and(|tick| tick >= effect.deadline_tick)
        );
        assert_eq!(request.mutation, None);
    }
    assert_eq!(f.reads.load(Ordering::SeqCst), 1);
    assert_eq!(
        f.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| matches!(request, DatasetRequest::EndBrowse { .. }))
            .count(),
        1
    );
}

#[test]
fn completed_readonly_duplicates_refuse_and_changed_identity_never_redispatches() {
    let f = fixture();
    invoke(&f, CicsOperation::StartBrowse, 1);
    let (effect, request) = offered(&f.actor, CicsOperation::ReadNext, 2);
    let first = f.service.invoke(&effect, request.clone()).unwrap();
    assert_eq!(first.payload.bytes(), b"AA11");
    assert_eq!(
        f.service.invoke(&effect, request.clone()),
        Err(HostProblem::UnknownOutcome)
    );
    let mut changed = request.clone();
    changed.arguments.insert(
        "RIDFLD".into(),
        BoundedPayload::new(
            "mainframe-env.cics.literal@1",
            b"BB".to_vec(),
            InvocationLimits::default(),
        )
        .unwrap(),
    );
    let mut changed_effect = effect.clone();
    changed_effect.request = HostRequest::Cics(changed.clone());
    assert_eq!(
        f.service.invoke(&changed_effect, changed),
        Err(HostProblem::IdempotencyConflict)
    );
    let mut changed_sequence = effect.clone();
    changed_sequence.sequence = 3;
    assert_eq!(
        f.service.invoke(&changed_sequence, request),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(f.reads.load(Ordering::SeqCst), 1);
    assert_eq!(
        invoke(&f, CicsOperation::ReadNext, 3).payload.bytes(),
        b"BB22"
    );
    invoke(&f, CicsOperation::EndBrowse, 4);
    let (effect, request) = offered(&f.actor, CicsOperation::EndBrowse, 4);
    assert_eq!(
        f.service.invoke(&effect, request),
        Err(HostProblem::UnknownOutcome)
    );
    assert_eq!(
        f.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| matches!(request, DatasetRequest::EndBrowse { .. }))
            .count(),
        1
    );
}

#[test]
fn initially_missing_receipt_is_rechecked_under_exclusive_command_lease() {
    let f = fixture();
    invoke(&f, CicsOperation::StartBrowse, 1);
    f.store.pause.store(true, Ordering::SeqCst);
    let service = f.service.clone();
    let (effect, request) = offered(&f.actor, CicsOperation::ReadNext, 2);
    std::thread::scope(|scope| {
        let waiting = scope.spawn(move || service.invoke(&effect, request));
        f.entered.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(
            invoke(&f, CicsOperation::ReadNext, 2).payload.bytes(),
            b"AA11"
        );
        f.release.send(()).unwrap();
        assert_eq!(waiting.join().unwrap(), Err(HostProblem::UnknownOutcome));
    });
    assert_eq!(f.reads.load(Ordering::SeqCst), 1);
    assert_eq!(
        invoke(&f, CicsOperation::ReadNext, 3).payload.bytes(),
        b"BB22"
    );
    invoke(&f, CicsOperation::EndBrowse, 4);
}

#[test]
fn persisted_unknown_readonly_reply_stays_protected_without_cursor_replay() {
    let f = fixture();
    invoke(&f, CicsOperation::StartBrowse, 1);
    f.service.inject_replay_unknown_after_persist_once();
    let (effect, request) = offered(&f.actor, CicsOperation::ReadNext, 2);
    assert_eq!(
        f.service.invoke(&effect, request.clone()),
        Err(HostProblem::UnknownOutcome)
    );
    let retained = f
        .store
        .get_provider_state("cics-effect-replay-v1", "original:2")
        .unwrap()
        .unwrap();
    assert_eq!(retained.version, 1);
    assert_eq!(
        f.service.invoke(&effect, request),
        Err(HostProblem::UnknownOutcome)
    );
    assert_eq!(
        f.store
            .get_provider_state("cics-effect-replay-v1", "original:2")
            .unwrap()
            .unwrap(),
        retained
    );
    assert_eq!(f.reads.load(Ordering::SeqCst), 1);
}

#[test]
fn failed_or_conflicting_postdispatch_readonly_retention_remains_unknown() {
    for conflict in [false, true] {
        let f = fixture();
        invoke(&f, CicsOperation::StartBrowse, 1);
        if conflict {
            f.store.conflict_write.store(true, Ordering::SeqCst);
        } else {
            f.store.fail_write.store(true, Ordering::SeqCst);
        }
        let (effect, request) = offered(&f.actor, CicsOperation::ReadNext, 2);
        assert_eq!(
            f.service.invoke(&effect, request),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(f.reads.load(Ordering::SeqCst), 1);
        assert_eq!(
            f.store
                .get_provider_state("cics-effect-replay-v1", "original:2")
                .unwrap(),
            None
        );
        // No direct retry: only the actual core Intent/Unknown fence qualifies
        // missing-provider-receipt recovery, exercised by coordinator controls.
    }
}
