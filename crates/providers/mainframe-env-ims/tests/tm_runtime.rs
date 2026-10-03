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

struct EnqueueThenUncertain {
    inner: Arc<MemoryStore>,
    fail: Mutex<bool>,
}

impl WorkStore for EnqueueThenUncertain {
    fn get_work(&self, work_id: &str) -> Result<Option<WorkRecord>, StoreError> {
        self.inner.get_work(work_id)
    }

    fn enqueue(&self, work: WorkRecord) -> Result<(), StoreError> {
        let mut fail = self.fail.lock().unwrap();
        if *fail {
            *fail = false;
            self.inner.enqueue(work)?;
            Err(StoreError::Infrastructure(
                "injected post-dispatch uncertainty".into(),
            ))
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
fn denied_tm_retry_cannot_observe_an_authorized_admission_receipt() {
    let policy = Arc::new(RecordingAuthorizer::default());
    let (service, _) = memory_service(policy.clone());
    let request = invocation("admission", "same-admission", 1_000);
    let first = service
        .enqueue(&request, message("message-replay", "PAY1", None))
        .unwrap();
    policy.deny("PAYPSB");
    assert_eq!(
        service.enqueue(&request, message("message-replay", "PAY1", None)),
        Err(HostProblem::Unauthorized)
    );
    assert_eq!(service.queued("PAY1", 10).unwrap(), vec![first.message_id]);
}

#[test]
fn admission_gap_repair_rollback_and_terminate_preserve_work_fences() {
    let policy = Arc::new(RecordingAuthorizer::default());
    let store = Arc::new(MemoryStore::new(StoreLimits::default()));
    let provider: Arc<dyn ProviderStateStore> = store.clone();
    let work: Arc<dyn WorkStore> = Arc::new(EnqueueThenUncertain {
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
        Err(HostProblem::UnknownOutcome)
    );
    assert_eq!(
        service.message_state("message-gap").unwrap(),
        Some(TmMessageState::AdmissionPending)
    );
    assert!(
        store
            .get_work("ims-tm:00000000000000000001:message-gap")
            .unwrap()
            .is_some()
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

#[test]
fn sqlite_reopen_preserves_package_bound_work_across_selection_rollback() {
    let directory = std::env::temp_dir().join(format!(
        "mainframe-env-ims-tm-package-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("package.sqlite");
    if database.exists() {
        std::fs::remove_file(&database).unwrap();
    }
    let url = format!("sqlite://{}?mode=rwc", database.display());
    let first_identity = format!("sha256:{:064x}", 1);
    let second_identity = format!("sha256:{:064x}", 2);
    let policy = Arc::new(RecordingAuthorizer::default());
    {
        let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let provider: Arc<dyn ProviderStateStore> = store.clone();
        let work: Arc<dyn WorkStore> = store;
        let service = TmService::open(provider, work, policy.clone(), TmLimits::default()).unwrap();
        let first = definitions();
        service
            .publish_package_definitions("GENERIC.APP", 1, &first_identity, Some(&first))
            .unwrap();
        let second = first.clone();
        service
            .publish_package_definitions("GENERIC.APP", 2, &second_identity, Some(&second))
            .unwrap();
        service
            .enqueue(
                &invocation("admission", "package-enqueue", 1_000),
                message("package-message", "PAY1", None),
            )
            .unwrap();
        service
            .publish_package_definitions("GENERIC.APP", 1, &first_identity, Some(&first))
            .unwrap();
        assert_eq!(
            service.publish_package_definitions("GENERIC.APP", 2, &first_identity, Some(&second)),
            Err(HostProblem::IdempotencyConflict)
        );
    }
    {
        let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let provider: Arc<dyn ProviderStateStore> = store.clone();
        let work: Arc<dyn WorkStore> = store;
        let service = TmService::open(provider, work, policy, TmLimits::default()).unwrap();
        assert!(
            service
                .selected_package_matches("GENERIC.APP", 1, &first_identity)
                .unwrap()
        );
        assert!(
            service
                .claim("PAY1", "selected-worker", 1, 40)
                .unwrap()
                .is_none()
        );
        let claimed = service
            .claim_retained(
                "GENERIC.APP",
                2,
                &second_identity,
                "PAY1",
                "retained-worker",
                1,
                40,
            )
            .unwrap()
            .unwrap();
        assert_eq!(service.package_for_work(&claimed).unwrap().generation, 2);
        service
            .start(&invocation("package-run", "package-start", 1_000), &claimed)
            .unwrap();
        assert_eq!(
            service
                .call(
                    &invocation("package-run", "package-gu", 1_000),
                    TmCall::GetUnique
                )
                .unwrap()
                .segment,
            Some(b"first".to_vec())
        );
        service
            .call(
                &invocation("package-run", "package-rollback", 1_000),
                TmCall::Rollback,
            )
            .unwrap();
    }
    std::fs::remove_dir_all(&directory).unwrap();
}

// Test-only source-backed gap: ordinary express TM PURG completes each group.
// No commit, next input, private row fabrication or recovery acceptance here.
fn public_express_purg_groups(
    provider: Arc<dyn ProviderStateStore>,
    work_store: Arc<dyn WorkStore>,
    second_group: bool,
) {
    let service = TmService::open(
        provider.clone(),
        work_store.clone(),
        Arc::new(RecordingAuthorizer::default()),
        TmLimits::default(),
    )
    .unwrap();
    let mut catalog = definitions();
    for transaction in &mut catalog.transactions {
        transaction.alternate_pcbs.push(TmAlternatePcbDefinition {
            name: "EXP".into(),
            destination: TmDestination::Fixed("TERM2".into()),
            express: true,
        });
    }
    service.install(catalog).unwrap();
    let admitted = enqueue(&service, "express-group-input", "PAY1", None);
    let work = service
        .claim("PAY1", "express-group-worker", 1, 50)
        .unwrap()
        .unwrap();
    assert_eq!(work.work_id, admitted.work_id);
    assert_eq!(work.state, WorkState::Claimed);
    assert!(work.lease_id.is_some());
    assert!(work.lease_epoch > 0);
    assert!(work.lease_expiry_tick.unwrap() > 1);
    service
        .start(
            &invocation("express-group-run", "express-start", 1000),
            &work,
        )
        .unwrap();
    let input = service
        .call(
            &invocation("express-group-run", "express-gu", 1000),
            TmCall::GetUnique,
        )
        .unwrap();
    assert_eq!(input.status, TmPcbStatus::SUCCESS);
    assert_eq!(input.segment, Some(b"first".to_vec()));
    for (key, segment) in [
        ("express-first-one", b"first-one".to_vec()),
        ("express-first-two", b"first-two".to_vec()),
    ] {
        assert_eq!(
            service
                .call(
                    &invocation("express-group-run", key, 1000),
                    TmCall::Insert {
                        pcb: TmPcb::Alternate("EXP".into()),
                        segment,
                    },
                )
                .unwrap()
                .status,
            TmPcbStatus::SUCCESS
        );
    }
    assert!(service.outbound("TERM2", 16).unwrap().is_empty());
    let purge = TmCall::Purge {
        pcb: TmPcb::Alternate("EXP".into()),
    };
    let first_invocation = invocation("express-group-run", "express-first-purg", 1000);
    let first = service.call(&first_invocation, purge.clone()).unwrap();
    assert_eq!(first.status, TmPcbStatus::SUCCESS);
    assert!(!first.replayed);
    assert_eq!(first.output_message_ids.len(), 1);
    let first_groups = service.outbound("TERM2", 16).unwrap();
    assert_eq!(first_groups.len(), 1);
    assert_eq!(first_groups[0].message_id, first.output_message_ids[0]);
    assert_eq!(first_groups[0].destination, "TERM2");
    assert!(first_groups[0].express);
    assert_eq!(
        first_groups[0].segments,
        vec![b"first-one".to_vec(), b"first-two".to_vec()]
    );
    let first_rows = provider
        .list_provider_state("ims-tm-v1-outbound", 16)
        .unwrap();
    assert_eq!(first_rows.len(), 1);
    let replay = service.call(&first_invocation, purge.clone()).unwrap();
    assert_eq!(replay.status, TmPcbStatus::SUCCESS);
    assert!(replay.replayed);
    assert_eq!(replay.output_message_ids, first.output_message_ids);
    assert_eq!(service.outbound("TERM2", 16).unwrap(), first_groups);
    assert_eq!(
        provider
            .list_provider_state("ims-tm-v1-outbound", 16)
            .unwrap(),
        first_rows
    );
    assert_eq!(work_store.get_work(&work.work_id).unwrap(), Some(work));
    eprintln!(
        "PUBLIC EXPRESS CONTROL: live enqueue/claim/start/GU; first literal group and exact PURG replay passed"
    );
    if second_group {
        for (key, segment) in [
            ("express-second-one", b"second-one".to_vec()),
            ("express-second-two", b"second-two".to_vec()),
        ] {
            assert_eq!(
                service
                    .call(
                        &invocation("express-group-run", key, 1000),
                        TmCall::Insert {
                            pcb: TmPcb::Alternate("EXP".into()),
                            segment,
                        },
                    )
                    .unwrap()
                    .status,
                TmPcbStatus::SUCCESS
            );
        }
        let second_invocation = invocation("express-group-run", "express-second-purg", 1000);
        assert_ne!(
            second_invocation.execution_id,
            first_invocation.execution_id
        );
        assert_ne!(
            second_invocation.idempotency_key,
            first_invocation.idempotency_key
        );
        let second = service.call(&second_invocation, purge);
        let actual_groups = service.outbound("TERM2", 16).unwrap();
        let actual_rows = provider
            .list_provider_state("ims-tm-v1-outbound", 16)
            .unwrap();
        eprintln!(
            "PUBLIC EXPRESS FAIL-FIRST: second={second:?}; stored_groups={actual_groups:?}; outbound_rows={actual_rows:?}"
        );
        assert_eq!(
            actual_rows.iter().find(|row| row.key == first_rows[0].key),
            Some(&first_rows[0]),
            "the completed first group must remain immutable"
        );
        assert_eq!(
            second.as_ref().map(|result| result.status),
            Ok(TmPcbStatus::SUCCESS)
        );
        let second = second.unwrap();
        assert_eq!(second.output_message_ids.len(), 1);
        assert_ne!(second.output_message_ids[0], first.output_message_ids[0]);
        assert_eq!(actual_groups.len(), 2);
        assert_eq!(
            actual_groups[0].segments,
            vec![b"first-one".to_vec(), b"first-two".to_vec()]
        );
        assert_eq!(
            actual_groups[1].segments,
            vec![b"second-one".to_vec(), b"second-two".to_vec()]
        );
        assert_eq!(actual_groups[1].message_id, second.output_message_ids[0]);
        assert!(actual_groups[1].sequence > actual_groups[0].sequence);
        assert_eq!(actual_rows.len(), 2);
    }
}

struct ExpressPurgSqliteDirectory(std::path::PathBuf);

impl Drop for ExpressPurgSqliteDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn public_express_purg_sqlite(second_group: bool) {
    let directory = ExpressPurgSqliteDirectory(std::env::temp_dir().join(format!(
        "ims-public-express-purg-{}-{second_group}",
        std::process::id()
    )));
    std::fs::create_dir(&directory.0).unwrap();
    let url = format!("sqlite:{}?mode=rwc", directory.0.join("state.db").display());
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    public_express_purg_groups(store.clone(), store, second_group);
}

#[test]
fn public_express_purg_output_identity_failfirst_memory() {
    let store = Arc::new(MemoryStore::new(StoreLimits::default()));
    public_express_purg_groups(store.clone(), store, true);
}

#[test]
fn public_express_purg_output_identity_failfirst_sqlite() {
    public_express_purg_sqlite(true);
}

#[test]
fn public_express_purg_first_group_replay_control_memory() {
    let store = Arc::new(MemoryStore::new(StoreLimits::default()));
    public_express_purg_groups(store.clone(), store, false);
}

#[test]
fn public_express_purg_first_group_replay_control_sqlite() {
    public_express_purg_sqlite(false);
}

// These explicit external-fixture entries are run with --ignored --exact and
// a required path. An unselected helper is never process or compatibility proof.
fn identity_definitions() -> TmDefinitionSet {
    let mut definitions = definitions();
    for transaction in &mut definitions.transactions {
        for name in ["EXP", "EXP2"] {
            transaction.alternate_pcbs.push(TmAlternatePcbDefinition {
                name: name.into(),
                destination: TmDestination::Fixed("TERM2".into()),
                express: true,
            });
        }
    }
    definitions
}

fn identity_open(store: Arc<dyn ProviderStateStore>, work: Arc<dyn WorkStore>) -> Arc<TmService> {
    TmService::open(
        store,
        work,
        Arc::new(RecordingAuthorizer::default()),
        TmLimits::default(),
    )
    .unwrap()
}

fn identity_start(service: &TmService, id: &str, key: &str) -> WorkRecord {
    enqueue(service, id, "PAY1", None);
    let work = service.claim("PAY1", key, 1, 500).unwrap().unwrap();
    assert_eq!(work.state, WorkState::Claimed);
    assert!(work.lease_id.is_some());
    service
        .start(
            &invocation("identity-run", &format!("{key}-start"), 1_000),
            &work,
        )
        .unwrap();
    assert_eq!(
        service
            .call(
                &invocation("identity-run", &format!("{key}-gu"), 1_000),
                TmCall::GetUnique
            )
            .unwrap()
            .segment,
        Some(b"first".to_vec())
    );
    work
}

fn identity_insert(service: &TmService, key: &str, pcb: &str, bytes: &[u8]) {
    let result = service
        .call(
            &invocation("identity-run", key, 1_000),
            TmCall::Insert {
                pcb: TmPcb::Alternate(pcb.into()),
                segment: bytes.to_vec(),
            },
        )
        .unwrap();
    assert_eq!(result.status, TmPcbStatus::SUCCESS);
    assert!(!result.replayed);
}

fn identity_purge(service: &TmService, key: &str, pcb: &str) -> mainframe_env_ims::TmCallResult {
    let result = service
        .call(
            &invocation("identity-run", key, 1_000),
            TmCall::Purge {
                pcb: TmPcb::Alternate(pcb.into()),
            },
        )
        .unwrap();
    assert_eq!(result.status, TmPcbStatus::SUCCESS);
    assert!(!result.replayed);
    assert_eq!(result.output_message_ids.len(), 1);
    result
}

fn identity_snapshot(store: &dyn ProviderStateStore) -> serde_json::Value {
    let mut rows = Vec::new();
    for namespace in [
        "ims-tm-v1-session",
        "ims-tm-v1-outbound",
        "ims-tm-v1-replay",
    ] {
        for row in store.list_provider_state(namespace, 1024).unwrap() {
            rows.push(serde_json::json!({"namespace": namespace, "key": row.key,
                "version": row.version, "payload": row.payload}));
        }
    }
    serde_json::Value::Array(rows)
}

fn identity_fixture() -> (std::path::PathBuf, Arc<SqliteStateStore>) {
    let path = std::path::PathBuf::from(
        std::env::var("IMS_TM_IDENTITY_FIXTURE").expect("explicit external fixture path required"),
    );
    let url = format!("sqlite:{}?mode=rwc", path.display());
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    (path, store)
}

#[test]
#[ignore = "explicit external base/new writer process"]
fn tm_identity_fixture_writer_process() {
    let (path, store) = identity_fixture();
    let service = identity_open(store.clone(), store.clone());
    service.install(identity_definitions()).unwrap();
    identity_start(&service, "compat-input", "compat");
    identity_insert(&service, "compat-express-insert", "EXP", b"legacy-express");
    let first = identity_purge(&service, "compat-express-purg", "EXP");
    identity_insert(
        &service,
        "compat-pending-insert",
        "FIXED",
        b"legacy-pending",
    );
    let pending = identity_purge(&service, "compat-pending-purg", "FIXED");
    assert_ne!(first.output_message_ids, pending.output_message_ids);
    let groups = service.outbound("TERM2", 10).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].segments, vec![b"legacy-express".to_vec()]);
    assert_eq!(
        store
            .list_provider_state("ims-tm-v1-outbound", 10)
            .unwrap()
            .len(),
        2
    );
    std::fs::write(
        path.with_extension("rows.json"),
        serde_json::to_vec_pretty(&identity_snapshot(store.as_ref())).unwrap(),
    )
    .unwrap();
    println!(
        "PHASE WRITER: real enqueue/live claim/start/GU; available express and pending ordinary; ids={:?}/{:?}",
        first.output_message_ids, pending.output_message_ids
    );
}

#[test]
#[ignore = "explicit external read-only compatibility process"]
fn tm_identity_fixture_readonly_process() {
    let (path, store) = identity_fixture();
    let before = identity_snapshot(store.as_ref());
    let expected: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path.with_extension("rows.json")).unwrap()).unwrap();
    assert_eq!(before, expected);
    let service = identity_open(store.clone(), store.clone());
    let groups = service.outbound("TERM2", 10).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].segments, vec![b"legacy-express".to_vec()]);
    let rows = store.list_provider_state("ims-tm-v1-outbound", 10).unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|row| {
        let value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        value["value"]["available"] == false
    }));
    assert_eq!(identity_snapshot(store.as_ref()), before);
    println!(
        "PHASE READONLY: genuine fixture opened; exact rows/versions/payloads preserved; available express and pending ordinary"
    );
}

#[test]
#[ignore = "explicit independent replay process"]
fn tm_identity_fixture_replay_process() {
    let (path, store) = identity_fixture();
    let expected: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path.with_extension("rows.json")).unwrap()).unwrap();
    assert_eq!(identity_snapshot(store.as_ref()), expected);
    let service = identity_open(store.clone(), store.clone());
    let first = service
        .call(
            &invocation("identity-run", "compat-express-purg", 1_000),
            TmCall::Purge {
                pcb: TmPcb::Alternate("EXP".into()),
            },
        )
        .unwrap();
    assert_eq!(first.status, TmPcbStatus::SUCCESS);
    assert!(first.replayed);
    assert_eq!(
        first.output_message_ids,
        vec![service.outbound("TERM2", 10).unwrap()[0].message_id.clone()]
    );
    assert_eq!(
        service.call(
            &invocation("identity-run", "compat-express-purg", 1_000),
            TmCall::Purge {
                pcb: TmPcb::Alternate("FIXED".into())
            }
        ),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(identity_snapshot(store.as_ref()), expected);
    println!(
        "PHASE REPLAY: exact old PURG after later state/reopen; changed PCB canonical conflict; no duplicate or rewind"
    );
}

fn identity_mixed_history(provider: Arc<dyn ProviderStateStore>, work_store: Arc<dyn WorkStore>) {
    let service = identity_open(provider.clone(), work_store.clone());
    service.install(identity_definitions()).unwrap();
    let initial_work = identity_start(&service, "mixed-input", "mixed");
    let mut immutable = Vec::new();
    for (insert, purg, bytes) in [
        ("mixed-a-in", "mixed-a-purg", b"first-express".as_slice()),
        ("mixed-b-in", "mixed-b-purg", b"second-express".as_slice()),
    ] {
        identity_insert(&service, insert, "EXP", bytes);
        identity_purge(&service, purg, "EXP");
        immutable = provider
            .list_provider_state("ims-tm-v1-outbound", 100)
            .unwrap();
    }
    let snapshot = identity_snapshot(provider.as_ref());
    let replay = service
        .call(
            &invocation("identity-run", "mixed-a-purg", 1_000),
            TmCall::Purge {
                pcb: TmPcb::Alternate("EXP".into()),
            },
        )
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(
        replay.output_message_ids,
        vec![
            service.outbound("TERM2", 100).unwrap()[0]
                .message_id
                .clone()
        ]
    );
    assert_eq!(
        service.call(
            &invocation("identity-run", "mixed-a-purg", 1_000),
            TmCall::Purge {
                pcb: TmPcb::Alternate("EXP2".into())
            }
        ),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(identity_snapshot(provider.as_ref()), snapshot);
    identity_insert(&service, "mixed-pending-in", "FIXED", b"pending-ordinary");
    let pending = identity_purge(&service, "mixed-pending-purg", "FIXED");
    let pending_before = provider
        .get_provider_state("ims-tm-v1-outbound", &pending.output_message_ids[0])
        .unwrap()
        .unwrap();
    assert_eq!(service.outbound("TERM2", 100).unwrap().len(), 2);
    identity_insert(&service, "mixed-c-in", "EXP2", b"third-express");
    identity_purge(&service, "mixed-c-purg", "EXP2");
    for (key, pcb, bytes) in [
        ("mixed-commit-a", "EXP", b"commit-exp".as_slice()),
        ("mixed-commit-b", "EXP2", b"commit-exp2".as_slice()),
        ("mixed-commit-c", "FIXED", b"commit-fixed".as_slice()),
    ] {
        identity_insert(&service, key, pcb, bytes);
    }
    service
        .call(
            &invocation("identity-run", "mixed-commit-io", 1_000),
            TmCall::Insert {
                pcb: TmPcb::Io,
                segment: b"commit-io".to_vec(),
            },
        )
        .unwrap();
    let result = service
        .call(
            &invocation("identity-run", "mixed-commit", 1_000),
            TmCall::Commit {
                conversation: Some(TmConversationAction::End),
            },
        )
        .unwrap();
    assert!(!result.replayed);
    assert_eq!(result.output_message_ids.len(), 5);
    assert_eq!(result.output_message_ids[0], pending.output_message_ids[0]);
    let groups = service.outbound("TERM2", 100).unwrap();
    assert_eq!(
        groups
            .iter()
            .map(|g| g.segments.clone())
            .collect::<Vec<_>>(),
        vec![
            vec![b"first-express".to_vec()],
            vec![b"second-express".to_vec()],
            vec![b"pending-ordinary".to_vec()],
            vec![b"third-express".to_vec()],
            vec![b"commit-exp".to_vec()],
            vec![b"commit-exp2".to_vec()],
            vec![b"commit-fixed".to_vec()],
        ]
    );
    assert!(groups.windows(2).all(|w| w[0].sequence < w[1].sequence));
    assert_eq!(
        groups
            .iter()
            .map(|group| group.sequence)
            .collect::<Vec<_>>(),
        vec![4, 6, 8, 10, 15, 16, 17]
    );
    assert_eq!(
        service.outbound("TERM1", 100).unwrap()[0].segments,
        vec![b"commit-io".to_vec()]
    );
    assert_eq!(service.outbound("TERM1", 100).unwrap()[0].sequence, 18);
    assert!(
        provider
            .get_provider_state("ims-tm-v1-session", "identity-run")
            .unwrap()
            .is_none()
    );
    for row in &immutable {
        assert_eq!(
            provider
                .get_provider_state(&row.namespace, &row.key)
                .unwrap()
                .as_ref(),
            Some(row)
        );
    }
    let pending_after = provider
        .get_provider_state("ims-tm-v1-outbound", &pending.output_message_ids[0])
        .unwrap()
        .unwrap();
    let before: serde_json::Value = serde_json::from_slice(&pending_before.payload).unwrap();
    let after: serde_json::Value = serde_json::from_slice(&pending_after.payload).unwrap();
    assert_eq!(before["value"]["message"], after["value"]["message"]);
    assert_eq!(after["value"]["available"], true);
    assert_eq!(pending_after.version, pending_before.version + 1);
    assert_eq!(
        work_store
            .get_work(&initial_work.work_id)
            .unwrap()
            .unwrap()
            .state,
        WorkState::Completed
    );

    let second_work = identity_start(&service, "reuse-input", "reuse");
    assert_ne!(second_work.work_id, initial_work.work_id);
    identity_insert(&service, "reuse-express-in", "EXP", b"reuse-express");
    let second = identity_purge(&service, "reuse-express-purg", "EXP");
    assert!(
        !groups
            .iter()
            .any(|g| second.output_message_ids.contains(&g.message_id))
    );
    let sent = provider
        .get_provider_state("ims-tm-v1-outbound", &second.output_message_ids[0])
        .unwrap()
        .unwrap();
    identity_insert(
        &service,
        "reuse-pending-in",
        "FIXED",
        b"discard-on-rollback",
    );
    let discarded = identity_purge(&service, "reuse-pending-purg", "FIXED");
    service
        .call(
            &invocation("identity-run", "reuse-rollback", 1_000),
            TmCall::Rollback,
        )
        .unwrap();
    assert_eq!(
        provider
            .get_provider_state(&sent.namespace, &sent.key)
            .unwrap()
            .as_ref(),
        Some(&sent)
    );
    assert!(
        provider
            .get_provider_state("ims-tm-v1-outbound", &discarded.output_message_ids[0])
            .unwrap()
            .is_none()
    );
    assert_eq!(
        work_store
            .get_work(&second_work.work_id)
            .unwrap()
            .unwrap()
            .state,
        WorkState::Queued
    );
    let reclaimed = service
        .claim("PAY1", "reclaimed-worker", 2, 500)
        .unwrap()
        .unwrap();
    assert_eq!(reclaimed.work_id, second_work.work_id);
    assert!(reclaimed.lease_epoch > second_work.lease_epoch);
    assert_ne!(reclaimed.lease_id, second_work.lease_id);
    service
        .start(
            &invocation("identity-run", "reclaimed-start", 1_000),
            &reclaimed,
        )
        .unwrap();
    identity_insert(&service, "reclaimed-in", "EXP", b"reclaimed-express");
    let third = identity_purge(&service, "reclaimed-purg", "EXP");
    assert_ne!(third.output_message_ids, second.output_message_ids);
    service
        .call(
            &invocation("identity-run", "reclaimed-commit", 1_000),
            TmCall::Commit {
                conversation: Some(TmConversationAction::End),
            },
        )
        .unwrap();
    identity_start(&service, "cancel-input", "cancel");
    identity_insert(&service, "cancel-exp-in", "EXP", b"cancel-express");
    let fourth = identity_purge(&service, "cancel-exp-purg", "EXP");
    let cancel_sent = provider
        .get_provider_state("ims-tm-v1-outbound", &fourth.output_message_ids[0])
        .unwrap()
        .unwrap();
    identity_insert(&service, "cancel-fixed-in", "FIXED", b"discard-on-cancel");
    let cancel_pending = identity_purge(&service, "cancel-fixed-purg", "FIXED");
    service
        .cancel(
            &invocation("identity-run", "cancel-call", 1_000),
            "cancel-input",
        )
        .unwrap();
    assert_eq!(
        provider
            .get_provider_state(&cancel_sent.namespace, &cancel_sent.key)
            .unwrap()
            .as_ref(),
        Some(&cancel_sent)
    );
    assert!(
        provider
            .get_provider_state("ims-tm-v1-outbound", &cancel_pending.output_message_ids[0])
            .unwrap()
            .is_none()
    );
    println!(
        "MIXED: strict same-incarnation order; commit slots, prior pending bytes; real new input/released-reclaimed epochs; express immutable through ordinary commit/rollback/cancel"
    );
}

#[test]
fn tm_output_identity_mixed_order_reuse_memory() {
    let store = Arc::new(MemoryStore::new(StoreLimits::default()));
    identity_mixed_history(store.clone(), store);
}

#[test]
fn tm_output_identity_mixed_order_reuse_sqlite() {
    let directory = ExpressPurgSqliteDirectory(
        std::env::temp_dir().join(format!("ims-identity-mixed-{}", std::process::id())),
    );
    std::fs::create_dir(&directory.0).unwrap();
    let url = format!("sqlite:{}?mode=rwc", directory.0.join("state.db").display());
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    identity_mixed_history(store.clone(), store);
}

fn identity_capacity(provider: Arc<dyn ProviderStateStore>, work: Arc<dyn WorkStore>) {
    let service = TmService::open(
        provider.clone(),
        work,
        Arc::new(RecordingAuthorizer::default()),
        TmLimits {
            max_outbound_messages: 1,
            ..TmLimits::default()
        },
    )
    .unwrap();
    service.install(identity_definitions()).unwrap();
    identity_start(&service, "capacity-input", "capacity");
    identity_insert(&service, "capacity-first", "EXP", b"immutable-first");
    identity_purge(&service, "capacity-purg", "EXP");
    identity_insert(&service, "capacity-second", "EXP", b"not-published");
    let before = identity_snapshot(provider.as_ref());
    assert_eq!(
        service.call(
            &invocation("identity-run", "capacity-reject", 1_000),
            TmCall::Purge {
                pcb: TmPcb::Alternate("EXP".into())
            }
        ),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(identity_snapshot(provider.as_ref()), before);
    assert_eq!(
        service.call(
            &invocation("identity-run", "capacity-commit-reject", 1_000),
            TmCall::Commit {
                conversation: Some(TmConversationAction::End)
            }
        ),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(identity_snapshot(provider.as_ref()), before);
    assert_eq!(
        service.outbound("TERM2", 1).unwrap()[0].segments,
        vec![b"immutable-first".to_vec()]
    );
}

#[test]
fn tm_output_identity_capacity_memory() {
    let store = Arc::new(MemoryStore::new(StoreLimits::default()));
    identity_capacity(store.clone(), store);
}

#[test]
fn tm_output_identity_capacity_sqlite() {
    let directory = ExpressPurgSqliteDirectory(
        std::env::temp_dir().join(format!("ims-identity-capacity-{}", std::process::id())),
    );
    std::fs::create_dir(&directory.0).unwrap();
    let url = format!("sqlite:{}?mode=rwc", directory.0.join("state.db").display());
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    identity_capacity(store.clone(), store);
}

enum IdentityPublicationFault {
    Before,
    After,
    Compete(Arc<std::sync::Barrier>),
}

struct IdentityPublicationStore {
    inner: Arc<dyn ProviderStateStore>,
    fault: Mutex<Option<IdentityPublicationFault>>,
}

impl mainframe_env_store_api::AuditSink for IdentityPublicationStore {
    fn record_audit(
        &self,
        record: mainframe_env_execution_api::AuditRecord,
    ) -> Result<(), StoreError> {
        self.inner.record_audit(record)
    }
    fn audit_records(
        &self,
        id: &ExecutionId,
        start: u64,
        max: usize,
    ) -> Result<Vec<mainframe_env_execution_api::AuditRecord>, StoreError> {
        self.inner.audit_records(id, start, max)
    }
}

impl ProviderStateStore for IdentityPublicationStore {
    fn advance_logical_clock(&self, floor: u64) -> Result<u64, StoreError> {
        self.inner.advance_logical_clock(floor)
    }
    fn get_provider_state(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<mainframe_env_store_api::ProviderStateRecord>, StoreError> {
        self.inner.get_provider_state(namespace, key)
    }
    fn list_provider_state(
        &self,
        namespace: &str,
        max: usize,
    ) -> Result<Vec<mainframe_env_store_api::ProviderStateRecord>, StoreError> {
        self.inner.list_provider_state(namespace, max)
    }
    fn list_provider_state_prefix(
        &self,
        prefix: &str,
        max: usize,
    ) -> Result<Vec<mainframe_env_store_api::ProviderStateRecord>, StoreError> {
        self.inner.list_provider_state_prefix(prefix, max)
    }
    fn put_provider_state(
        &self,
        record: mainframe_env_store_api::ProviderStateRecord,
        expected: Option<u64>,
    ) -> Result<(), StoreError> {
        self.inner.put_provider_state(record, expected)
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
        record: mainframe_env_store_api::ProviderStateRecord,
        old: &str,
        expected: u64,
    ) -> Result<(), StoreError> {
        self.inner.move_provider_state(record, old, expected)
    }
    fn put_provider_states_atomic(
        &self,
        writes: Vec<mainframe_env_store_api::ProviderStateWrite>,
    ) -> Result<(), StoreError> {
        self.inner.put_provider_states_atomic(writes)
    }
    fn mutate_provider_states_atomic(
        &self,
        mutations: Vec<mainframe_env_store_api::ProviderStateMutation>,
    ) -> Result<(), StoreError> {
        let publication = mutations.iter().any(|m| matches!(m, mainframe_env_store_api::ProviderStateMutation::Put(w) if w.record.namespace == "ims-tm-v1-outbound"));
        let fault = if publication {
            self.fault.lock().unwrap().take()
        } else {
            None
        };
        match fault {
            Some(IdentityPublicationFault::Before) => {
                Err(StoreError::Infrastructure("before publication".into()))
            }
            Some(IdentityPublicationFault::After) => {
                self.inner.mutate_provider_states_atomic(mutations)?;
                Err(StoreError::Infrastructure(
                    "lost publication acknowledgement".into(),
                ))
            }
            Some(IdentityPublicationFault::Compete(barrier)) => {
                barrier.wait();
                self.inner.mutate_provider_states_atomic(mutations)
            }
            None => self.inner.mutate_provider_states_atomic(mutations),
        }
    }
}

fn identity_fault_history(provider: Arc<dyn ProviderStateStore>, work: Arc<dyn WorkStore>) {
    let fault_store = Arc::new(IdentityPublicationStore {
        inner: provider.clone(),
        fault: Mutex::new(None),
    });
    let service = identity_open(fault_store.clone(), work.clone());
    service.install(identity_definitions()).unwrap();
    identity_start(&service, "fault-input", "fault");
    identity_insert(&service, "fault-pre-in", "EXP", b"pre-failure-retry");
    let before = identity_snapshot(provider.as_ref());
    *fault_store.fault.lock().unwrap() = Some(IdentityPublicationFault::Before);
    let request = invocation("identity-run", "fault-pre-purg", 1_000);
    let call = TmCall::Purge {
        pcb: TmPcb::Alternate("EXP".into()),
    };
    assert_eq!(
        service.call(&request, call.clone()),
        Err(HostProblem::UnknownOutcome)
    );
    assert_eq!(identity_snapshot(provider.as_ref()), before);
    let first = service.call(&request, call.clone()).unwrap();
    assert_eq!(first.status, TmPcbStatus::SUCCESS);
    assert!(!first.replayed);
    identity_insert(
        &service,
        "fault-post-in",
        "EXP",
        b"post-failure-exactly-once",
    );
    let before_post = identity_snapshot(provider.as_ref());
    *fault_store.fault.lock().unwrap() = Some(IdentityPublicationFault::After);
    let post_request = invocation("identity-run", "fault-post-purg", 1_000);
    assert_eq!(
        service.call(&post_request, call.clone()),
        Err(HostProblem::UnknownOutcome)
    );
    let after = identity_snapshot(provider.as_ref());
    assert_ne!(before_post, after);
    assert_eq!(
        provider
            .list_provider_state("ims-tm-v1-outbound", 100)
            .unwrap()
            .len(),
        2
    );
    let reopened = identity_open(provider.clone(), work.clone());
    let retry = reopened.call(&post_request, call).unwrap();
    assert_eq!(retry.status, TmPcbStatus::SUCCESS);
    assert!(retry.replayed);
    assert_ne!(retry.output_message_ids, first.output_message_ids);
    assert_eq!(identity_snapshot(provider.as_ref()), after);
    assert_eq!(
        reopened
            .outbound("TERM2", 100)
            .unwrap()
            .iter()
            .map(|g| g.segments.clone())
            .collect::<Vec<_>>(),
        vec![
            vec![b"pre-failure-retry".to_vec()],
            vec![b"post-failure-exactly-once".to_vec()]
        ]
    );

    identity_insert(&reopened, "fault-cas-in", "EXP", b"one-cas-winner");
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let competitors = ["fault-cas-a", "fault-cas-b"].map(|key| {
        let wrapper = Arc::new(IdentityPublicationStore {
            inner: provider.clone(),
            fault: Mutex::new(Some(IdentityPublicationFault::Compete(barrier.clone()))),
        });
        let competitor = identity_open(wrapper, work.clone());
        std::thread::spawn(move || {
            (
                key,
                competitor.call(
                    &invocation("identity-run", key, 1_000),
                    TmCall::Purge {
                        pcb: TmPcb::Alternate("EXP".into()),
                    },
                ),
            )
        })
    });
    let outcomes = competitors.map(|thread| thread.join().unwrap());
    assert_eq!(outcomes.iter().filter(|(_, r)| r.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|(_, r)| *r == Err(HostProblem::IdempotencyConflict))
            .count(),
        1
    );
    for (key, result) in &outcomes {
        assert_eq!(
            provider
                .get_provider_state("ims-tm-v1-replay", key)
                .unwrap()
                .is_some(),
            result.is_ok()
        );
    }
    assert_eq!(reopened.outbound("TERM2", 100).unwrap().len(), 3);
    let winner = outcomes.iter().find(|(_, r)| r.is_ok()).unwrap();
    let exact = reopened
        .call(
            &invocation("identity-run", winner.0, 1_000),
            TmCall::Purge {
                pcb: TmPcb::Alternate("EXP".into()),
            },
        )
        .unwrap();
    assert!(exact.replayed);
    assert_eq!(
        exact.output_message_ids,
        winner.1.as_ref().unwrap().output_message_ids
    );
    let current = identity_snapshot(provider.as_ref());
    let replay_count = provider
        .list_provider_state("ims-tm-v1-replay", 1024)
        .unwrap()
        .len();
    let bounded = TmService::open(
        provider.clone(),
        work,
        Arc::new(RecordingAuthorizer::default()),
        TmLimits {
            max_replays: replay_count,
            ..TmLimits::default()
        },
    )
    .unwrap();
    assert_eq!(
        bounded.call(
            &invocation("identity-run", "fault-replay-full", 1_000),
            TmCall::Purge {
                pcb: TmPcb::Alternate("EXP".into())
            }
        ),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(identity_snapshot(provider.as_ref()), current);
    println!(
        "FAULTS: pre/post publication UnknownOutcome; retry/reopen exact once; two live CAS writers one winner/no loser replay; retained replay capacity fails closed"
    );
}

#[test]
fn tm_output_identity_atomic_failures_cas_memory() {
    let store = Arc::new(MemoryStore::new(StoreLimits::default()));
    identity_fault_history(store.clone(), store);
}

#[test]
fn tm_output_identity_atomic_failures_cas_sqlite() {
    let directory = ExpressPurgSqliteDirectory(
        std::env::temp_dir().join(format!("ims-identity-fault-{}", std::process::id())),
    );
    std::fs::create_dir(&directory.0).unwrap();
    let url = format!("sqlite:{}?mode=rwc", directory.0.join("state.db").display());
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    identity_fault_history(store.clone(), store);
}

#[test]
#[ignore = "explicit independent lost-acknowledgement writer process"]
fn tm_identity_lost_ack_writer_process() {
    let (path, store) = identity_fixture();
    let wrapper = Arc::new(IdentityPublicationStore {
        inner: store.clone(),
        fault: Mutex::new(None),
    });
    let service = identity_open(wrapper.clone(), store.clone());
    service.install(identity_definitions()).unwrap();
    identity_start(&service, "lost-ack-input", "lost-ack");
    identity_insert(&service, "lost-ack-in", "EXP", b"lost-ack-literal");
    *wrapper.fault.lock().unwrap() = Some(IdentityPublicationFault::After);
    assert_eq!(
        service.call(
            &invocation("identity-run", "lost-ack-purg", 1_000),
            TmCall::Purge {
                pcb: TmPcb::Alternate("EXP".into())
            }
        ),
        Err(HostProblem::UnknownOutcome)
    );
    let groups = service.outbound("TERM2", 10).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].segments, vec![b"lost-ack-literal".to_vec()]);
    assert!(
        store
            .get_provider_state("ims-tm-v1-replay", "lost-ack-purg")
            .unwrap()
            .is_some()
    );
    std::fs::write(
        path.with_extension("rows.json"),
        serde_json::to_vec_pretty(&identity_snapshot(store.as_ref())).unwrap(),
    )
    .unwrap();
    println!(
        "PHASE LOST ACK WRITER: actual atomic publication then UnknownOutcome, no in-process retry; id={}",
        groups[0].message_id
    );
}

#[test]
#[ignore = "explicit independent lost-acknowledgement retry process"]
fn tm_identity_lost_ack_replay_process() {
    let (path, store) = identity_fixture();
    let expected: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path.with_extension("rows.json")).unwrap()).unwrap();
    assert_eq!(identity_snapshot(store.as_ref()), expected);
    let service = identity_open(store.clone(), store.clone());
    let before = service.outbound("TERM2", 10).unwrap();
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].segments, vec![b"lost-ack-literal".to_vec()]);
    let request = invocation("identity-run", "lost-ack-purg", 1_000);
    let retry = service
        .call(
            &request,
            TmCall::Purge {
                pcb: TmPcb::Alternate("EXP".into()),
            },
        )
        .unwrap();
    assert_eq!(retry.status, TmPcbStatus::SUCCESS);
    assert!(retry.replayed);
    assert_eq!(retry.output_message_ids, vec![before[0].message_id.clone()]);
    assert_eq!(
        service.call(
            &request,
            TmCall::Purge {
                pcb: TmPcb::Alternate("EXP2".into())
            }
        ),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(service.outbound("TERM2", 10).unwrap(), before);
    assert_eq!(identity_snapshot(store.as_ref()), expected);
    println!(
        "PHASE LOST ACK REPLAY: independent SQLite reopen/retry exact retained ID, bytes and rows; changed canonical conflicts; no partial or duplicate group"
    );
}
