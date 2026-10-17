//! Actual compiled sequential I/O and genuine provider journal/replay boundaries.
use mainframe_env_dataset::{DatasetLimits, DatasetService, dataset_providers};
use mainframe_env_execution_api::{
    AuditDecision, AuditRecord, BoundedPayload, CapabilityId, ExecutionOutcome, Invocation,
    InvocationLimits, LifecycleEvent, LifecycleEventKind, Machine, MachineDrive, MachineResume,
    Principal, Quantum,
};
use mainframe_env_host_api::{
    CapabilityDescriptor, DatasetAttributes, DatasetName, DatasetOrganization, DatasetRequest,
    DatasetResult, EffectRequest, EffectResult, HostLimits, HostProblem, HostProvider, HostRequest,
    HostResult, KeyRelation, Mutation, RecordFormat, RegistrySnapshot, ScopedHostService,
    canonical_audit_resource_digest, canonical_request_digest, canonical_result_digest,
};
use mainframe_env_interpreter::{
    CoordinatorLimits, ExecutionControl, ExecutionCoordinator, ReferenceMachine,
};
use mainframe_env_ir::CodecLimits;
use mainframe_env_store::{MemoryStore, SqliteStateStore, StoreLimits};
use mainframe_env_store_api::{
    CheckpointRecord, EffectRecord, EffectState, ExecutionRecord, ExecutionState, OutboxRecord,
    PlatformStore, ProviderStateRecord, StoreError,
};
use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};
use std::sync::{Arc, Mutex};

const SOURCE: &str = "IDENTIFICATION DIVISION. PROGRAM-ID. CURSORIO.
ENVIRONMENT DIVISION. INPUT-OUTPUT SECTION. FILE-CONTROL.
SELECT INPUT-FILE ASSIGN TO INPUT-DD ORGANIZATION IS SEQUENTIAL
ACCESS MODE IS SEQUENTIAL FILE STATUS IO-STATUS.
DATA DIVISION. FILE SECTION. FD INPUT-FILE. 01 INPUT-RECORD PIC X(2).
WORKING-STORAGE SECTION. 01 IO-STATUS PIC XX.
PROCEDURE DIVISION.
OPEN INPUT INPUT-FILE. DISPLAY IO-STATUS.
READ INPUT-FILE. DISPLAY INPUT-RECORD. DISPLAY IO-STATUS.
READ INPUT-FILE. DISPLAY INPUT-RECORD. DISPLAY IO-STATUS.
READ INPUT-FILE. DISPLAY IO-STATUS.
CLOSE INPUT-FILE. DISPLAY IO-STATUS. STOP RUN.";

fn dataset() -> DatasetName {
    DatasetName::new("TEST.INPUT", 128).unwrap()
}

fn key(actor: &Invocation, sequence: u64) -> mainframe_env_execution_api::IdempotencyKey {
    mainframe_env_execution_api::IdempotencyKey::new(
        format!("{}:{sequence}", actor.idempotency_key.as_str()),
        InvocationLimits::default(),
    )
    .unwrap()
}

fn seed(service: &DatasetService, organization: DatasetOrganization) {
    let mutation = |sequence| Mutation {
        sequence,
        idempotency_key: mainframe_env_execution_api::IdempotencyKey::new(
            format!("seed-{sequence}"),
            InvocationLimits::default(),
        )
        .unwrap(),
        transaction: None,
    };
    service
        .invoke(DatasetRequest::Create {
            dataset: dataset(),
            attributes: DatasetAttributes {
                organization,
                record_format: RecordFormat::Fixed,
                logical_record_length: 2,
                key_offset: (organization == DatasetOrganization::KeySequenced).then_some(0),
                key_length: (organization == DatasetOrganization::KeySequenced).then_some(2),
                ccsid: None,
            },
            mutation: mutation(1),
        })
        .unwrap();
    service
        .invoke(DatasetRequest::Write {
            dataset: dataset(),
            member: None,
            records: vec![b"AA".to_vec(), b"BB".to_vec()],
            expected_version: Some(1),
            mutation: mutation(2),
        })
        .unwrap();
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct JournalFootprint {
    execution: Option<ExecutionRecord>,
    events: Vec<LifecycleEvent>,
    effects: Vec<Option<EffectRecord>>,
    audits: Result<Vec<AuditRecord>, StoreError>,
    notifications: Vec<OutboxRecord>,
    checkpoint: Option<CheckpointRecord>,
    datasets: Vec<ProviderStateRecord>,
    dataset_replay: Vec<ProviderStateRecord>,
}

#[derive(Clone, Copy)]
struct ReadLimits {
    audits: usize,
    outbox: usize,
}
impl Default for ReadLimits {
    fn default() -> Self {
        Self {
            audits: 16,
            outbox: 64,
        }
    }
}

fn footprint(
    store: &dyn PlatformStore,
    actor: &Invocation,
    limits: ReadLimits,
) -> JournalFootprint {
    JournalFootprint {
        execution: store.get_execution(&actor.execution_id).unwrap(),
        events: store.events(&actor.execution_id, 0, 64).unwrap(),
        effects: (1..=5)
            .map(|sequence| store.effect(&key(actor, sequence)).unwrap())
            .collect(),
        // A deliberately zero audit quota admits no positive reader bound. Retain
        // its actual CapacityExceeded observation, rather than inventing empty rows.
        audits: store.audit_records(&actor.execution_id, 0, limits.audits.max(1)),
        notifications: store.pending_notifications(limits.outbox).unwrap(),
        checkpoint: store.get_checkpoint(&actor.execution_id).unwrap(),
        datasets: store.list_provider_state("dataset", 16).unwrap(),
        dataset_replay: store.list_provider_state("dataset-replay", 16).unwrap(),
    }
}

#[derive(Clone)]
struct Call {
    request: EffectRequest,
    result: EffectResult,
    before_result_commit: JournalFootprint,
}
type Calls = Arc<Mutex<Vec<Call>>>;

struct ActualProvider {
    inner: Arc<dyn HostProvider>,
    store: Arc<dyn PlatformStore>,
    calls: Calls,
    require_fresh_intent: bool,
    read_limits: ReadLimits,
}
impl HostProvider for ActualProvider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        self.inner.descriptor()
    }
    fn invoke(&self, actor: &Invocation, request: EffectRequest) -> EffectResult {
        if self.require_fresh_intent
            && let Some(key) = &request.idempotency_key
        {
            let intent = self.store.effect(key).unwrap().unwrap();
            assert_eq!(intent.state, EffectState::Intent);
            assert_eq!(intent.execution_id, actor.execution_id);
            assert_eq!(intent.run_unit_id, actor.run_unit_id);
            assert_eq!(intent.sequence, request.sequence);
            assert_eq!(
                intent.request_digest,
                canonical_request_digest(&request.request).unwrap()
            );
        }
        // The real provider owns every reply and cursor change. This wrapper only observes.
        let result = self.inner.invoke(actor, request.clone());
        let before_result_commit = footprint(self.store.as_ref(), actor, self.read_limits);
        self.calls.lock().unwrap().push(Call {
            request,
            result: result.clone(),
            before_result_commit,
        });
        result
    }
}

fn coordinator(
    store: Arc<dyn PlatformStore>,
    service: Arc<DatasetService>,
    calls: Calls,
    require_fresh_intent: bool,
    read_limits: ReadLimits,
) -> ExecutionCoordinator {
    let actual = dataset_providers(service, InvocationLimits::default())
        .into_iter()
        .find(|provider| provider.descriptor().capability.as_str() == "host.dataset.read")
        .unwrap();
    let observed: Arc<dyn HostProvider> = Arc::new(ActualProvider {
        inner: actual,
        store: store.clone(),
        calls,
        require_fresh_intent,
        read_limits,
    });
    let host = Arc::new(ScopedHostService::new(
        Arc::new(RegistrySnapshot::new(1, vec![observed], InvocationLimits::default()).unwrap()),
        HostLimits::default(),
    ));
    ExecutionCoordinator::durable(host, store, CoordinatorLimits::default())
}

struct Fixture {
    store: Arc<dyn PlatformStore>,
    service: Arc<DatasetService>,
    coordinator: ExecutionCoordinator,
    calls: Calls,
    actor: Invocation,
    machine: ReferenceMachine,
    read_limits: ReadLimits,
}
fn fixture(store: Arc<dyn PlatformStore>, source: &str) -> Fixture {
    fixture_with_queries(
        store,
        source,
        DatasetOrganization::Sequential,
        ReadLimits::default(),
    )
}
fn fixture_with_queries(
    store: Arc<dyn PlatformStore>,
    source: &str,
    organization: DatasetOrganization,
    read_limits: ReadLimits,
) -> Fixture {
    let service = DatasetService::open(store.clone(), DatasetLimits::default()).unwrap();
    seed(&service, organization);
    let artifact = crate::compile(source).unwrap();
    let mut actor = crate::invocation(&artifact, 4096);
    actor.principal = Principal::new(
        actor.principal.id().clone(),
        BTreeSet::from([
            CapabilityId::new("host.dataset.read", InvocationLimits::default()).unwrap(),
        ]),
        InvocationLimits::default(),
    )
    .unwrap();
    actor.bindings.insert(
        "cobol.dd.INPUT-DD".into(),
        BoundedPayload::new(
            "mainframe-env.dataset-name@1",
            b"TEST.INPUT".to_vec(),
            InvocationLimits::default(),
        )
        .unwrap(),
    );
    let machine =
        ReferenceMachine::from_binary(artifact.payload(), actor.clone(), CodecLimits::default())
            .unwrap();
    let calls = Calls::default();
    let coordinator = coordinator(
        store.clone(),
        service.clone(),
        calls.clone(),
        true,
        read_limits,
    );
    Fixture {
        store,
        service,
        coordinator,
        calls,
        actor,
        machine,
        read_limits,
    }
}

fn control(
    tick: &mut u64,
) -> Result<ExecutionControl, mainframe_env_interpreter::ExecutionControlError> {
    *tick += 1;
    Ok(ExecutionControl {
        now_tick: *tick,
        cancellation_requested: false,
    })
}
fn unknown(outcome: &ExecutionOutcome) -> bool {
    matches!(outcome, ExecutionOutcome::ProviderFailure(problem) if problem.has_unknown_outcome())
}

fn successful_compiled_io(store: Arc<dyn PlatformStore>) {
    let mut f = fixture(store, SOURCE);
    let initial = f.store.list_provider_state("dataset", 16).unwrap();
    let mut tick = 0;
    let outcome = f
        .coordinator
        .execute_with_control(&mut f.machine, &f.actor, || control(&mut tick));
    assert!(
        matches!(outcome, ExecutionOutcome::Completed(done) if done.return_code == 0 && done.output.bytes() == b"00\nAA\n00\nBB\n00\n10\n00\n")
    );
    let calls = f.calls.lock().unwrap();
    assert_eq!(calls.len(), 5);
    let mut pairs = Vec::new();
    for event in f.store.events(&f.actor.execution_id, 0, 32).unwrap() {
        match event.kind {
            LifecycleEventKind::EffectIntent { sequence } => pairs.push((false, sequence)),
            LifecycleEventKind::EffectResult { sequence } => pairs.push((true, sequence)),
            _ => {}
        }
    }
    assert_eq!(
        pairs,
        [
            (false, 1),
            (true, 1),
            (false, 2),
            (true, 2),
            (false, 3),
            (true, 3),
            (false, 4),
            (true, 4),
            (false, 5),
            (true, 5)
        ]
    );
    let audits = f.store.audit_records(&f.actor.execution_id, 0, 16).unwrap();
    assert_eq!(audits.len(), 5);
    for (index, call) in calls.iter().enumerate() {
        let sequence = index as u64 + 1;
        assert_eq!(call.request.sequence, sequence);
        assert_eq!(call.request.idempotency_key, Some(key(&f.actor, sequence)));
        assert!(!call.request.request.is_mutating());
        assert_eq!(call.request.request.mutation(), None);
        let effect = f.store.effect(&key(&f.actor, sequence)).unwrap().unwrap();
        assert_eq!(effect.state, EffectState::Completed);
        assert_eq!(effect.execution_id, f.actor.execution_id);
        assert_eq!(effect.run_unit_id, f.actor.run_unit_id);
        assert_eq!(effect.intent.owner, f.actor.execution_id);
        assert_eq!(effect.intent.attempt, f.actor.attempt);
        assert_eq!(
            effect.intent.audit_invocation_key,
            Some(f.actor.idempotency_key.clone())
        );
        assert_eq!(
            effect.intent.capability.as_ref().unwrap().as_str(),
            "host.dataset.read"
        );
        assert_eq!(
            effect.intent.audit_resource,
            Some(canonical_audit_resource_digest(&call.request.request))
        );
        assert_eq!(
            effect.request_digest,
            canonical_request_digest(&call.request.request).unwrap()
        );
        assert_eq!(
            effect.result_digest,
            Some(canonical_result_digest(&call.result.outcome).unwrap())
        );
        assert!(
            effect
                .resolved_tick
                .is_some_and(|tick| tick >= effect.intent.created_tick && tick > 0)
        );
        assert_eq!(audits[index].decision, AuditDecision::Success);
        assert_eq!(audits[index].effect_sequence, sequence);
    }
    assert!(matches!(
        &calls[0].request.request,
        HostRequest::Dataset(DatasetRequest::StartBrowse { .. })
    ));
    assert!(matches!(
        &calls[3].result.outcome,
        Ok(HostResult::Dataset(DatasetResult::Browse {
            record: None,
            ..
        }))
    ));
    assert!(matches!(
        &calls[4].request.request,
        HostRequest::Dataset(DatasetRequest::Close {
            cursor: Some(_),
            ..
        })
    ));
    assert_eq!(f.store.list_provider_state("dataset", 16).unwrap(), initial);
}

#[test]
fn memory_compiled_cursor_io_records_genuine_results_eof_and_close() {
    successful_compiled_io(Arc::new(MemoryStore::new(StoreLimits::default())));
}

#[test]
fn sqlite_compiled_cursor_io_records_genuine_results_eof_and_close() {
    successful_compiled_io(Arc::new(
        SqliteStateStore::open("sqlite::memory:", 1_048_576, 4096).unwrap(),
    ));
}

#[derive(Debug)]
struct InterruptedAfterResult;
struct Interrupt<'a> {
    inner: &'a mut ReferenceMachine,
    sequence: u64,
}
impl Machine for Interrupt<'_> {
    type Effect = EffectRequest;
    type EffectResult = EffectResult;
    fn drive(
        &mut self,
        resume: MachineResume<EffectResult>,
        quantum: Quantum,
    ) -> MachineDrive<EffectRequest> {
        if matches!(&resume, MachineResume::HostResult(result) if result.sequence == self.sequence)
        {
            // Simulate driver loss only after the real coordinator result commit returned.
            panic_any(InterruptedAfterResult);
        }
        self.inner.drive(resume, quantum)
    }
}
struct Repeat(Option<EffectRequest>);
impl Machine for Repeat {
    type Effect = EffectRequest;
    type EffectResult = EffectResult;
    fn drive(&mut self, _: MachineResume<EffectResult>, _: Quantum) -> MachineDrive<EffectRequest> {
        MachineDrive::HostCall(
            self.0
                .take()
                .expect("cursor replay must refuse before machine resume"),
        )
    }
}

fn start(service: &DatasetService) -> String {
    match service
        .invoke(DatasetRequest::StartBrowse {
            dataset: dataset(),
            key: Vec::new(),
            relation: KeyRelation::GreaterOrEqual,
        })
        .unwrap()
    {
        DatasetResult::Browse { cursor, .. } => cursor,
        other => panic!("real cursor required: {other:?}"),
    }
}
fn next(service: &DatasetService, cursor: &str) -> Result<DatasetResult, HostProblem> {
    service.invoke(DatasetRequest::ReadNext {
        dataset: dataset(),
        cursor: cursor.into(),
        reverse: false,
        control: Default::default(),
    })
}

fn completed_cursor_replay(sqlite: bool) {
    // OPEN, first READ and CLOSE exercise allocation, gap advancement and removal.
    for sequence in [1, 2, 5] {
        for reopen in [false, true] {
            // Each iteration gets independent execution/provider identities and rows.
            let fresh: Arc<dyn PlatformStore> = if sqlite {
                Arc::new(SqliteStateStore::open("sqlite::memory:", 1_048_576, 4096).unwrap())
            } else {
                Arc::new(MemoryStore::new(Default::default()))
            };
            let mut f = fixture(fresh, SOURCE);
            let mut tick = 0;
            let interrupted = catch_unwind(AssertUnwindSafe(|| {
                f.coordinator.execute_with_control(
                    &mut Interrupt {
                        inner: &mut f.machine,
                        sequence,
                    },
                    &f.actor,
                    || control(&mut tick),
                )
            }));
            assert!(interrupted.unwrap_err().is::<InterruptedAfterResult>());
            assert_eq!(
                f.store
                    .get_execution(&f.actor.execution_id)
                    .unwrap()
                    .unwrap()
                    .state,
                ExecutionState::Running
            );
            let request = f.calls.lock().unwrap().last().unwrap().request.clone();
            let original = f.store.effect(&key(&f.actor, sequence)).unwrap().unwrap();
            assert_eq!(original.state, EffectState::Completed);
            let service = if reopen {
                DatasetService::open(f.store.clone(), DatasetLimits::default()).unwrap()
            } else {
                f.service.clone()
            };
            if reopen {
                assert_eq!(start(&service), "cursor-1");
            }
            let calls = Calls::default();
            // Resume observation must not itself block a buggy second provider call.
            let resumed = coordinator(
                f.store.clone(),
                service.clone(),
                calls.clone(),
                false,
                f.read_limits,
            );
            let result = resumed.execute_resumable_with_control(
                &mut Repeat(Some(request)),
                &f.actor,
                || control(&mut tick),
            );
            assert!(unknown(&result));
            assert!(calls.lock().unwrap().is_empty());
            assert_eq!(
                f.store.effect(&key(&f.actor, sequence)).unwrap(),
                Some(original)
            );
            let cursor = "cursor-1";
            if reopen || sequence == 1 {
                assert!(
                    matches!(next(&service, cursor), Ok(DatasetResult::Browse { record: Some(record), .. }) if record == b"AA")
                );
            } else if sequence == 2 {
                assert!(
                    matches!(next(&service, cursor), Ok(DatasetResult::Browse { record: Some(record), .. }) if record == b"BB")
                );
            } else {
                assert!(next(&service, cursor).is_err());
            }
            assert_eq!(start(&service), "cursor-2");
        }
    }
}

#[test]
fn completed_cursor_replay_refuses_before_live_or_reopened_provider_state_moves() {
    completed_cursor_replay(false);
}

#[test]
fn sqlite_completed_cursor_replay_refuses_before_live_or_reopened_provider_state_moves() {
    completed_cursor_replay(true);
}

#[test]
fn completed_cursor_replay_preserves_current_control_and_identity_refusal_precedence() {
    for refusal in 0..5 {
        let mut f = fixture(Arc::new(MemoryStore::new(Default::default())), SOURCE);
        let mut tick = 0;
        let interrupted = catch_unwind(AssertUnwindSafe(|| {
            f.coordinator.execute_with_control(
                &mut Interrupt {
                    inner: &mut f.machine,
                    sequence: 1,
                },
                &f.actor,
                || control(&mut tick),
            )
        }));
        assert!(interrupted.unwrap_err().is::<InterruptedAfterResult>());
        let mut request = f.calls.lock().unwrap()[0].request.clone();
        let original = f.store.effect(&key(&f.actor, 1)).unwrap().unwrap();
        if refusal == 3 {
            request.sequence += 1;
        } else if refusal == 4 {
            let HostRequest::Dataset(DatasetRequest::StartBrowse { key, .. }) =
                &mut request.request
            else {
                panic!("original OPEN required")
            };
            key.push(b'A');
        }
        let calls = Calls::default();
        let resumed = coordinator(
            f.store.clone(),
            f.service.clone(),
            calls.clone(),
            false,
            f.read_limits,
        );
        let mut observations = 0;
        let result =
            resumed.execute_resumable_with_control(&mut Repeat(Some(request)), &f.actor, || {
                observations += 1;
                if observations == 3 && refusal == 2 {
                    return Err(mainframe_env_interpreter::ExecutionControlError::Unavailable);
                }
                tick += 1;
                Ok(ExecutionControl {
                    now_tick: if observations == 3 && refusal == 1 {
                        f.actor.deadline_tick
                    } else {
                        tick
                    },
                    cancellation_requested: observations == 3 && refusal == 0,
                })
            });
        match refusal {
            0 => assert!(matches!(result, ExecutionOutcome::Cancelled)),
            1 => assert!(matches!(result, ExecutionOutcome::TimedOut)),
            2 => assert!(matches!(result, ExecutionOutcome::InfrastructureFailure(_))),
            _ => assert!(unknown(&result)),
        }
        assert!(calls.lock().unwrap().is_empty());
        assert_eq!(f.store.effect(&key(&f.actor, 1)).unwrap(), Some(original));
        assert!(
            matches!(next(&f.service, "cursor-1"), Ok(DatasetResult::Browse { record: Some(record), .. }) if record == b"AA")
        );
        assert_eq!(start(&f.service), "cursor-2");
    }
}

#[test]
fn cursor_result_quota_failures_restore_the_actual_journal_and_keep_original_intent() {
    for sequence in 1..=5 {
        for audit_quota in [true, false] {
            let limits = if audit_quota {
                StoreLimits {
                    max_audits: sequence - 1,
                    ..StoreLimits::default()
                }
            } else {
                StoreLimits {
                    max_outbox: 2 + sequence * 2,
                    ..StoreLimits::default()
                }
            };
            let read_limits = ReadLimits {
                audits: limits.max_audits.min(16),
                outbox: limits.max_outbox.min(64),
            };
            let mut f = fixture_with_queries(
                Arc::new(MemoryStore::new(limits)),
                SOURCE,
                DatasetOrganization::Sequential,
                read_limits,
            );
            let mut tick = 0;
            let result = f
                .coordinator
                .execute_with_control(&mut f.machine, &f.actor, || control(&mut tick));
            assert!(unknown(&result));
            let calls = f.calls.lock().unwrap();
            assert_eq!(calls.len(), sequence);
            let last = calls.last().unwrap().clone();
            drop(calls);
            // Actual post-provider/pre-commit state equals the rollback result, including
            // the current Intent, prior completed effects, event/outbox/audit and raw rows.
            assert_eq!(
                footprint(f.store.as_ref(), &f.actor, f.read_limits),
                last.before_result_commit
            );
            match &last.before_result_commit.audits {
                Err(StoreError::CapacityExceeded) if read_limits.audits == 0 => {}
                Ok(rows) => assert_eq!(rows.len(), sequence - 1),
                other => panic!("unexpected quota-bounded audit observation: {other:?}"),
            }
            let original = f
                .store
                .effect(&key(&f.actor, sequence as u64))
                .unwrap()
                .unwrap();
            assert_eq!(original.state, EffectState::Intent);
            assert_eq!(original.result_digest, None);
            assert_eq!(original.resolved_tick, None);
            let mut repeat = Repeat(Some(last.request));
            let retry = f
                .coordinator
                .execute_resumable_with_control(&mut repeat, &f.actor, || control(&mut tick));
            if audit_quota {
                // Resume admission fits; dispatch sees the genuine unresolved Intent.
                assert!(repeat.0.is_none());
                assert!(unknown(&retry));
            } else {
                // The full outbox refuses Resumed before the machine can offer a call.
                assert!(matches!(retry, ExecutionOutcome::InfrastructureFailure(_)));
                assert!(repeat.0.is_some());
                assert_eq!(
                    footprint(f.store.as_ref(), &f.actor, f.read_limits),
                    last.before_result_commit
                );
            }
            assert_eq!(f.calls.lock().unwrap().len(), sequence);
            assert_eq!(
                f.store.effect(&key(&f.actor, sequence as u64)).unwrap(),
                Some(original)
            );
        }
    }
}

#[test]
fn cursor_intent_quota_refuses_before_allocating_the_real_cursor() {
    let mut f = fixture(
        Arc::new(MemoryStore::new(StoreLimits {
            max_effects: 0,
            ..StoreLimits::default()
        })),
        SOURCE,
    );
    let mut tick = 0;
    assert!(matches!(
        f.coordinator
            .execute_with_control(&mut f.machine, &f.actor, || control(&mut tick)),
        ExecutionOutcome::InfrastructureFailure(_)
    ));
    assert!(f.calls.lock().unwrap().is_empty());
    assert_eq!(f.store.effect(&key(&f.actor, 1)).unwrap(), None);
    assert_eq!(start(&f.service), "cursor-1");
}

#[test]
fn compiled_non_cursor_open_close_keep_unkeyed_readonly_compatibility() {
    let source = SOURCE
        .replace("ORGANIZATION IS SEQUENTIAL", "ORGANIZATION IS INDEXED")
        .replace(
            "ACCESS MODE IS SEQUENTIAL",
            "ACCESS MODE IS RANDOM RECORD KEY IS INPUT-RECORD",
        )
        .replace(
            "READ INPUT-FILE. DISPLAY INPUT-RECORD. DISPLAY IO-STATUS.\n",
            "",
        )
        .replace("READ INPUT-FILE. DISPLAY IO-STATUS.\n", "");
    let mut f = fixture_with_queries(
        Arc::new(MemoryStore::new(Default::default())),
        &source,
        DatasetOrganization::KeySequenced,
        ReadLimits::default(),
    );
    let before = f.store.list_provider_state("dataset", 16).unwrap();
    let mut tick = 0;
    assert!(
        matches!(f.coordinator.execute_with_control(&mut f.machine, &f.actor, || control(&mut tick)), ExecutionOutcome::Completed(done) if done.output.bytes() == b"00\n00\n")
    );
    let calls = f.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert!(matches!(
        &calls[0].request.request,
        HostRequest::Dataset(DatasetRequest::Attributes { .. })
    ));
    assert!(matches!(
        &calls[1].request.request,
        HostRequest::Dataset(DatasetRequest::Close { cursor: None, .. })
    ));
    assert!(
        calls
            .iter()
            .all(|call| call.request.idempotency_key.is_none())
    );
    assert_eq!(f.store.effect(&key(&f.actor, 1)).unwrap(), None);
    assert_eq!(f.store.effect(&key(&f.actor, 2)).unwrap(), None);
    assert_eq!(f.store.list_provider_state("dataset", 16).unwrap(), before);
}
