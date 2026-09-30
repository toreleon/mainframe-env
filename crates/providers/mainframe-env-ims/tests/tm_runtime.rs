use mainframe_env_execution_api::{
    ArtifactRef, CancellationProbe, ExecutionId, IdempotencyKey, Invocation, InvocationLimits,
    Principal, PrincipalId, RequestId, ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
};
use mainframe_env_host_api::{
    EnterpriseAuthorizer, EnterpriseResource, HostProblem, ImsPcbKind, ImsStatusContext,
    resolve_ims_status,
};
use mainframe_env_ims::{
    TmAlternatePcbDefinition, TmCall, TmConversationAction, TmDefinitionSet, TmDestination,
    TmEnqueueReceipt, TmExecutionContext, TmInputMessage, TmLimits, TmMessageState, TmPcb,
    TmPcbStatus, TmPcbView, TmService, TmTransactionDefinition,
};
use mainframe_env_store::{MemoryStore, SqliteStateStore, StoreLimits};
use mainframe_env_store_api::{ProviderStateStore, StoreError, WorkRecord, WorkState, WorkStore};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct RecordingAuthorizer {
    denied: Mutex<BTreeSet<String>>,
    seen: Mutex<Vec<EnterpriseResource>>,
}

struct FailFirstEnqueue {
    inner: Arc<MemoryStore>,
    fail: Mutex<bool>,
}

impl WorkStore for FailFirstEnqueue {
    fn get_work(&self, work_id: &str) -> Result<Option<WorkRecord>, StoreError> {
        self.inner.get_work(work_id)
    }

    fn enqueue(&self, work: WorkRecord) -> Result<(), StoreError> {
        let mut fail = self.fail.lock().unwrap();
        if *fail {
            *fail = false;
            Err(StoreError::Infrastructure("injected enqueue gap".into()))
        } else {
            self.inner.enqueue(work)
        }
    }

    fn claim(
        &self,
        worker: &str,
        required_generation: Option<&str>,
        now_tick: u64,
        lease_ticks: u64,
    ) -> Result<Option<WorkRecord>, StoreError> {
        self.inner
            .claim(worker, required_generation, now_tick, lease_ticks)
    }

    fn heartbeat(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
        lease_ticks: u64,
    ) -> Result<WorkRecord, StoreError> {
        self.inner
            .heartbeat(work_id, lease_id, lease_epoch, now_tick, lease_ticks)
    }

    fn release(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
        available_tick: u64,
    ) -> Result<WorkRecord, StoreError> {
        self.inner
            .release(work_id, lease_id, lease_epoch, now_tick, available_tick)
    }

    fn request_cancellation(&self, work_id: &str) -> Result<WorkRecord, StoreError> {
        self.inner.request_cancellation(work_id)
    }

    fn dead_letter(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
    ) -> Result<WorkRecord, StoreError> {
        self.inner
            .dead_letter(work_id, lease_id, lease_epoch, now_tick)
    }

    fn complete(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
    ) -> Result<(), StoreError> {
        self.inner
            .complete(work_id, lease_id, lease_epoch, now_tick)
    }
}

impl RecordingAuthorizer {
    fn deny(&self, name: &str) {
        self.denied.lock().unwrap().insert(name.into());
    }

    fn allow(&self, name: &str) {
        self.denied.lock().unwrap().remove(name);
    }
}

impl EnterpriseAuthorizer for RecordingAuthorizer {
    fn authorize(&self, _: &PrincipalId, resource: &EnterpriseResource) -> Result<(), HostProblem> {
        self.seen.lock().unwrap().push(resource.clone());
        if self.denied.lock().unwrap().contains(resource.name.as_str()) {
            Err(HostProblem::Unauthorized)
        } else {
            Ok(())
        }
    }
}

fn definitions() -> TmDefinitionSet {
    TmDefinitionSet {
        transactions: ["PAY1", "PAY2"]
            .into_iter()
            .map(|code| TmTransactionDefinition {
                code: code.into(),
                psb: "PAYPSB".into(),
                program_selector: format!("ims:{code}"),
                artifact: format!("artifact:{code}"),
                required_generation: "ims-generation-1".into(),
                context: TmExecutionContext::MessageProcessing,
                priority: 7,
                timeout_ticks: 100,
                conversational: true,
                spa_size: 64,
                alternate_pcbs: vec![
                    TmAlternatePcbDefinition {
                        name: "FIXED".into(),
                        destination: TmDestination::Fixed("TERM2".into()),
                        express: false,
                    },
                    TmAlternatePcbDefinition {
                        name: "ROUTE".into(),
                        destination: TmDestination::Modifiable,
                        express: true,
                    },
                ],
            })
            .collect(),
    }
}

fn message(id: &str, transaction: &str, conversation_id: Option<String>) -> TmInputMessage {
    TmInputMessage {
        message_id: id.into(),
        transaction: transaction.into(),
        source: "TERM1".into(),
        user_id: Some("ALICE".into()),
        group_name: Some("PAYROLL".into()),
        conversation_id,
        segments: vec![b"first".to_vec(), b"second".to_vec()],
    }
}

fn invocation(run: &str, key: &str, deadline: u64) -> Invocation {
    let limits = InvocationLimits::default();
    Invocation::new(
        RequestId::new(format!("request-{key}"), limits).unwrap(),
        ExecutionId::new(format!("execution-{key}"), limits).unwrap(),
        RunUnitId::new(run, limits).unwrap(),
        None,
        Selector::new("ims:tm", limits).unwrap(),
        ArtifactRef::new("artifact:tm", limits).unwrap(),
        Principal::new(
            PrincipalId::new("ALICE", limits).unwrap(),
            BTreeSet::new(),
            limits,
        )
        .unwrap(),
        ServiceClass::Interactive,
        7,
        deadline,
        TraceId::new(format!("trace-{key}"), limits).unwrap(),
        IdempotencyKey::new(key, limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .unwrap()
}

fn memory_service(policy: Arc<RecordingAuthorizer>) -> (Arc<TmService>, Arc<MemoryStore>) {
    let store = Arc::new(MemoryStore::new(StoreLimits::default()));
    let provider: Arc<dyn ProviderStateStore> = store.clone();
    let work: Arc<dyn WorkStore> = store.clone();
    let service = TmService::open(provider, work, policy, TmLimits::default()).unwrap();
    service.install(definitions()).unwrap();
    (service, store)
}

fn enqueue(
    service: &TmService,
    id: &str,
    transaction: &str,
    conversation_id: Option<String>,
) -> TmEnqueueReceipt {
    service
        .enqueue(
            &invocation("admission", &format!("enqueue-{id}"), 1_000),
            message(id, transaction, conversation_id),
        )
        .unwrap()
}

#[test]
fn runtime_statuses_resolve_for_message_io_context() {
    for status in [
        TmPcbStatus::SUCCESS,
        TmPcbStatus::NO_MORE_MESSAGES,
        TmPcbStatus::NO_MORE_SEGMENTS,
        TmPcbStatus::INVALID_CALL,
        TmPcbStatus::INVALID_SEGMENT_LENGTH,
    ] {
        assert!(
            resolve_ims_status(
                status.as_str().as_bytes(),
                ImsStatusContext::Message,
                ImsPcbKind::Io,
            )
            .is_ok(),
            "{} is missing from the message I/O PCB registry",
            status.as_str()
        );
        let encoded = serde_json::to_value(status).unwrap();
        assert_eq!(
            serde_json::from_value::<TmPcbStatus>(encoded).unwrap(),
            status
        );
    }
    assert!(
        resolve_ims_status(
            TmPcbStatus::SUCCESS.as_str().as_bytes(),
            ImsStatusContext::Message,
            ImsPcbKind::Alternate,
        )
        .is_ok()
    );
}

#[test]
fn output_segment_capacity_rejects_before_persisting() {
    let policy = Arc::new(RecordingAuthorizer::default());
    let store = Arc::new(MemoryStore::new(StoreLimits::default()));
    let provider: Arc<dyn ProviderStateStore> = store.clone();
    let work: Arc<dyn WorkStore> = store;
    let limits = TmLimits {
        max_segments_per_message: 1,
        ..TmLimits::default()
    };
    let service = TmService::open(provider, work, policy, limits).unwrap();
    service.install(definitions()).unwrap();
    let mut input = message("bounded", "PAY1", None);
    input.segments.truncate(1);
    service
        .enqueue(&invocation("admission", "bounded-enqueue", 1_000), input)
        .unwrap();
    let claimed = service.claim("PAY1", "worker-1", 1, 20).unwrap().unwrap();
    service
        .start(&invocation("bounded-run", "bounded-start", 1_000), &claimed)
        .unwrap();
    let segment = TmCall::Insert {
        pcb: TmPcb::Io,
        segment: b"first".to_vec(),
    };
    service
        .call(
            &invocation("bounded-run", "bounded-first", 1_000),
            segment.clone(),
        )
        .unwrap();
    assert_eq!(
        service.call(&invocation("bounded-run", "bounded-second", 1_000), segment),
        Err(HostProblem::ResourceExhausted)
    );
    assert!(service.outbound("TERM1", 10).unwrap().is_empty());
    service
        .call(
            &invocation("bounded-run", "bounded-commit", 1_000),
            TmCall::Commit {
                conversation: Some(TmConversationAction::End),
            },
        )
        .unwrap();
    assert_eq!(service.outbound("TERM1", 10).unwrap()[0].segments.len(), 1);
}

#[test]
fn queue_order_schedule_alternate_destinations_and_replay_are_exact() {
    let policy = Arc::new(RecordingAuthorizer::default());
    let (service, store) = memory_service(policy);

    let first = enqueue(&service, "message-1", "PAY1", None);
    let second = enqueue(&service, "message-2", "PAY1", None);
    assert!(first.sequence < second.sequence);
    assert_eq!(
        service.queued("PAY1", 10).unwrap(),
        vec!["message-1", "message-2"]
    );
    let replay = service
        .enqueue(
            &invocation("admission", "enqueue-message-1", 1_000),
            message("message-1", "PAY1", None),
        )
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.sequence, first.sequence);

    let claimed = service.claim("PAY1", "worker-1", 1, 20).unwrap().unwrap();
    assert_eq!(claimed.work_id, first.work_id);
    assert_eq!(claimed.effect_sequence, first.sequence);
    let scheduled = service
        .start(&invocation("run-1", "start-1", 1_000), &claimed)
        .unwrap();
    assert_eq!(scheduled.message_id, "message-1");
    assert_eq!(scheduled.spa, Vec::<u8>::new());

    let gu = service
        .call(&invocation("run-1", "gu-1", 1_000), TmCall::GetUnique)
        .unwrap();
    assert_eq!(
        (gu.status, gu.segment),
        (TmPcbStatus::SUCCESS, Some(b"first".to_vec()))
    );
    assert_eq!(
        gu.pcb,
        Some(TmPcbView::Io {
            logical_terminal: "TERM1".into(),
            status: TmPcbStatus::SUCCESS,
            message_sequence: first.sequence,
            user_id: Some("ALICE".into()),
            group_name: Some("PAYROLL".into()),
        })
    );
    let gn = service
        .call(&invocation("run-1", "gn-1", 1_000), TmCall::GetNext)
        .unwrap();
    assert_eq!(gn.segment, Some(b"second".to_vec()));
    let end = service
        .call(&invocation("run-1", "gn-end", 1_000), TmCall::GetNext)
        .unwrap();
    assert_eq!(end.status, TmPcbStatus::NO_MORE_SEGMENTS);

    service
        .call(
            &invocation("run-1", "change-1", 1_000),
            TmCall::Change {
                pcb: "ROUTE".into(),
                destination: "PAY2".into(),
            },
        )
        .unwrap();
    let insert_invocation = invocation("run-1", "insert-1", 1_000);
    service
        .call(
            &insert_invocation,
            TmCall::Insert {
                pcb: TmPcb::Alternate("ROUTE".into()),
                segment: b"out-one".to_vec(),
            },
        )
        .unwrap();
    let insert_replay = service
        .call(
            &insert_invocation,
            TmCall::Insert {
                pcb: TmPcb::Alternate("ROUTE".into()),
                segment: b"out-one".to_vec(),
            },
        )
        .unwrap();
    assert!(insert_replay.replayed);
    service
        .call(
            &invocation("run-1", "insert-2", 1_000),
            TmCall::Insert {
                pcb: TmPcb::Alternate("ROUTE".into()),
                segment: b"out-two".to_vec(),
            },
        )
        .unwrap();
    let purged = service
        .call(
            &invocation("run-1", "purge-1", 1_000),
            TmCall::Purge {
                pcb: TmPcb::Alternate("ROUTE".into()),
            },
        )
        .unwrap();
    assert_eq!(purged.destination.as_deref(), Some("PAY2"));
    assert_eq!(service.outbound("PAY2", 10).unwrap()[0].segments.len(), 2);

    let committed = service
        .call(
            &invocation("run-1", "commit-1", 1_000),
            TmCall::Commit {
                conversation: Some(TmConversationAction::Continue {
                    spa: b"step-one".to_vec(),
                }),
            },
        )
        .unwrap();
    assert_eq!(committed.conversation_id, first.conversation_id);
    assert_eq!(
        store.get_work(&first.work_id).unwrap().unwrap().state,
        WorkState::Completed
    );
    assert_eq!(service.queued("PAY1", 10).unwrap(), vec!["message-2"]);
}

#[test]
fn authorization_timeout_and_live_cancellation_precede_session_mutation() {
    let policy = Arc::new(RecordingAuthorizer::default());
    policy.deny("PAYPSB");
    let (service, store) = memory_service(policy.clone());
    assert_eq!(
        service.enqueue(
            &invocation("admission", "denied-enqueue", 1_000),
            message("denied", "PAY1", None),
        ),
        Err(HostProblem::Unauthorized)
    );
    assert!(service.queued("PAY1", 10).unwrap().is_empty());
    policy.allow("PAYPSB");

    let receipt = enqueue(&service, "message-1", "PAY1", None);
    let claimed = service.claim("PAY1", "worker-1", 1, 20).unwrap().unwrap();
    service
        .start(&invocation("run-1", "start-1", 1_000), &claimed)
        .unwrap();

    store.advance_logical_clock(20).unwrap();
    assert_eq!(
        service.call(&invocation("run-1", "timed-out", 10), TmCall::GetUnique),
        Err(HostProblem::TimedOut)
    );
    let probe = CancellationProbe::new();
    probe.request();
    assert_eq!(
        service.call(
            &invocation("run-1", "cancelled-call", 1_000).with_cancellation_probe(probe),
            TmCall::GetUnique,
        ),
        Err(HostProblem::Cancelled)
    );
    let first = service
        .call(&invocation("run-1", "gu-after", 1_000), TmCall::GetUnique)
        .unwrap();
    assert_eq!(first.segment, Some(b"first".to_vec()));

    policy.deny("DEST.PAY2");
    assert_eq!(
        service.call(
            &invocation("run-1", "denied-change", 1_000),
            TmCall::Change {
                pcb: "ROUTE".into(),
                destination: "PAY2".into(),
            },
        ),
        Err(HostProblem::Unauthorized)
    );
    assert_eq!(
        service
            .call(
                &invocation("run-1", "insert-after-deny", 1_000),
                TmCall::Insert {
                    pcb: TmPcb::Alternate("ROUTE".into()),
                    segment: b"not-routed".to_vec(),
                },
            )
            .unwrap()
            .status,
        TmPcbStatus::INVALID_CALL
    );

    let queued = enqueue(&service, "message-2", "PAY1", None);
    let cancelled = service
        .cancel(
            &invocation("admission", "cancel-message-2", 1_000),
            "message-2",
        )
        .unwrap();
    assert_eq!(cancelled.state, TmMessageState::Cancelled);
    assert_eq!(
        store.get_work(&queued.work_id).unwrap().unwrap().state,
        WorkState::Cancelled
    );
    assert_eq!(
        service.message_state("message-2").unwrap(),
        Some(TmMessageState::Cancelled)
    );
    assert_eq!(
        service.message_state("message-1").unwrap(),
        Some(TmMessageState::InFlight)
    );
    service
        .cancel(
            &invocation("admission", "cancel-message-1", 1_000),
            "message-1",
        )
        .unwrap();
    assert_eq!(
        service.message_state("message-1").unwrap(),
        Some(TmMessageState::Cancelled)
    );
    let provider: Arc<dyn ProviderStateStore> = store.clone();
    let work: Arc<dyn WorkStore> = store.clone();
    TmService::open(provider, work, policy, TmLimits::default()).unwrap();
    assert_eq!(receipt.message_id, "message-1");
}

#[test]
fn admission_gap_repair_rollback_and_terminate_preserve_work_fences() {
    let policy = Arc::new(RecordingAuthorizer::default());
    let store = Arc::new(MemoryStore::new(StoreLimits::default()));
    let provider: Arc<dyn ProviderStateStore> = store.clone();
    let work: Arc<dyn WorkStore> = Arc::new(FailFirstEnqueue {
        inner: store.clone(),
        fail: Mutex::new(true),
    });
    let service = TmService::open(provider, work, policy, TmLimits::default()).unwrap();
    service.install(definitions()).unwrap();

    assert_eq!(
        service.enqueue(
            &invocation("admission", "enqueue-gap", 1_000),
            message("message-gap", "PAY1", None),
        ),
        Err(HostProblem::InfrastructureFailure)
    );
    assert_eq!(
        service.message_state("message-gap").unwrap(),
        Some(TmMessageState::AdmissionPending)
    );
    assert_eq!(service.repair_schedules(1).unwrap(), 1);
    assert_eq!(
        service.message_state("message-gap").unwrap(),
        Some(TmMessageState::Scheduled)
    );
    let recovered = service
        .enqueue(
            &invocation("admission", "enqueue-gap", 1_000),
            message("message-gap", "PAY1", None),
        )
        .unwrap();
    assert_eq!(recovered.sequence, 1);

    let first_claim = service.claim("PAY1", "worker-1", 1, 50).unwrap().unwrap();
    service
        .start(&invocation("run-gap-1", "start-gap-1", 1_000), &first_claim)
        .unwrap();
    service
        .call(
            &invocation("run-gap-1", "insert-gap", 1_000),
            TmCall::Insert {
                pcb: TmPcb::Alternate("FIXED".into()),
                segment: b"rolled-back".to_vec(),
            },
        )
        .unwrap();
    service
        .call(
            &invocation("run-gap-1", "purge-gap", 1_000),
            TmCall::Purge {
                pcb: TmPcb::Alternate("FIXED".into()),
            },
        )
        .unwrap();
    assert!(service.outbound("TERM2", 10).unwrap().is_empty());
    service
        .call(
            &invocation("run-gap-1", "rollback-gap", 1_000),
            TmCall::Rollback,
        )
        .unwrap();
    assert_eq!(
        store.get_work(&first_claim.work_id).unwrap().unwrap().state,
        WorkState::Queued
    );
    assert!(service.outbound("TERM2", 10).unwrap().is_empty());

    let retry = service.claim("PAY1", "worker-2", 2, 50).unwrap().unwrap();
    service
        .start(&invocation("run-gap-2", "start-gap-2", 1_000), &retry)
        .unwrap();
    service
        .call(
            &invocation("run-gap-2", "terminate-gap", 1_000),
            TmCall::Terminate,
        )
        .unwrap();
    assert_eq!(
        store.get_work(&retry.work_id).unwrap().unwrap().state,
        WorkState::Completed
    );
    assert_eq!(
        service.message_state("message-gap").unwrap(),
        Some(TmMessageState::Completed)
    );
}

#[test]
fn sqlite_reopen_preserves_cursor_output_and_conversation_continuation() {
    let directory = std::env::temp_dir().join(format!(
        "mainframe-env-ims-tm-runtime-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("ims-tm.sqlite");
    if database.exists() {
        std::fs::remove_file(&database).unwrap();
    }
    let url = format!("sqlite://{}?mode=rwc", database.display());
    let limits = TmLimits::default();
    let policy = Arc::new(RecordingAuthorizer::default());

    let conversation_id;
    {
        let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let provider: Arc<dyn ProviderStateStore> = store.clone();
        let work: Arc<dyn WorkStore> = store.clone();
        let service = TmService::open(provider, work, policy.clone(), limits).unwrap();
        service.install(definitions()).unwrap();
        let receipt = enqueue(&service, "message-1", "PAY1", None);
        conversation_id = receipt.conversation_id.clone().unwrap();
        let claimed = service.claim("PAY1", "worker-1", 1, 50).unwrap().unwrap();
        service
            .start(&invocation("run-1", "start-1", 1_000), &claimed)
            .unwrap();
        service
            .call(&invocation("run-1", "gu-1", 1_000), TmCall::GetUnique)
            .unwrap();
        service
            .call(
                &invocation("run-1", "fixed-output", 1_000),
                TmCall::Insert {
                    pcb: TmPcb::Alternate("FIXED".into()),
                    segment: b"before-reopen".to_vec(),
                },
            )
            .unwrap();
    }

    {
        let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let provider: Arc<dyn ProviderStateStore> = store.clone();
        let work: Arc<dyn WorkStore> = store.clone();
        let service = TmService::open(provider, work, policy.clone(), limits).unwrap();
        let replayed = service
            .call(&invocation("run-1", "gu-1", 1_000), TmCall::GetUnique)
            .unwrap();
        assert!(replayed.replayed);
        assert_eq!(replayed.segment, Some(b"first".to_vec()));
        let next = service
            .call(
                &invocation("run-1", "gn-after-reopen", 1_000),
                TmCall::GetNext,
            )
            .unwrap();
        assert_eq!(next.segment, Some(b"second".to_vec()));
        service
            .call(
                &invocation("run-1", "commit-after-reopen", 1_000),
                TmCall::Commit {
                    conversation: Some(TmConversationAction::Switch {
                        transaction: "PAY2".into(),
                        spa: b"continued".to_vec(),
                    }),
                },
            )
            .unwrap();
        assert_eq!(
            service.outbound("TERM2", 10).unwrap()[0].segments,
            vec![b"before-reopen".to_vec()]
        );

        enqueue(&service, "message-2", "PAY2", Some(conversation_id.clone()));
        let claimed = service.claim("PAY2", "worker-2", 2, 50).unwrap().unwrap();
        let resumed = service
            .start(&invocation("run-2", "start-2", 1_000), &claimed)
            .unwrap();
        assert_eq!(resumed.spa, b"continued");
        service
            .call(
                &invocation("run-2", "end-conversation", 1_000),
                TmCall::Commit {
                    conversation: Some(TmConversationAction::End),
                },
            )
            .unwrap();
        assert_eq!(service.conversation(&conversation_id).unwrap(), None);
    }
    std::fs::remove_file(database).unwrap();
    std::fs::remove_dir(directory).unwrap();
}
