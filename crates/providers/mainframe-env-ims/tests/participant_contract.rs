//! Supplemental local evidence only: pending IMS is never admitted to a shared UOW.
use mainframe_env_execution_api::{
    ArtifactRef, CapabilityId, ExecutionId, IdempotencyKey, Invocation, InvocationLimits,
    ParticipantStatus, Principal, PrincipalId, RequestId, ResourceLimits, RunUnitId, Selector,
    ServiceClass, TraceId, transaction_participant_contract_v1,
};
use mainframe_env_host_api::{
    EffectRequest, EnterpriseAuthorizer, EnterpriseResource, HostProblem, HostRequest, HostResult,
    ImsOperation, ImsRequest, ImsResult, Mutation,
};
use mainframe_env_ims::*;
use mainframe_env_store::{MemoryStore, SqliteStateStore};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Authorization {
    deny: AtomicBool,
    fail: AtomicBool,
    seen: Mutex<Vec<(PrincipalId, EnterpriseResource)>>,
}

impl EnterpriseAuthorizer for Authorization {
    fn authorize(
        &self,
        principal: &PrincipalId,
        resource: &EnterpriseResource,
    ) -> Result<(), HostProblem> {
        self.seen
            .lock()
            .unwrap()
            .push((principal.clone(), resource.clone()));
        if self.deny.load(Ordering::SeqCst) {
            Err(HostProblem::Unauthorized)
        } else if self.fail.load(Ordering::SeqCst) {
            Err(HostProblem::InfrastructureFailure)
        } else {
            Ok(())
        }
    }
}

struct ReplayClock(AtomicBool);

impl ImsReplayClock for ReplayClock {
    fn now_tick(&self) -> Result<u64, HostProblem> {
        if self.0.load(Ordering::SeqCst) {
            Err(HostProblem::InfrastructureFailure)
        } else {
            Ok(250)
        }
    }
}

fn catalog() -> ImsMetadataCatalog {
    ImsMetadataCatalog {
        schema_version: IMS_METADATA_SCHEMA_V1.into(),
        databases: vec![ImsDatabaseMetadata {
            name: "BINDDB".into(),
            version: 1,
            organization: ImsDatabaseOrganization::Hidam,
            segments: vec![ImsSegmentMetadata {
                name: "ROOT".into(),
                parent: None,
                min_length: 3,
                max_length: 3,
                fields: vec![ImsFieldMetadata {
                    name: Some("KEY".into()),
                    offset: 0,
                    length: 2,
                    sequence: true,
                    unique: true,
                }],
            }],
            secondary_indexes: vec![],
            logical_relationships: vec![],
        }],
        psbs: vec![ImsPsbMetadata {
            name: "BINDPSB".into(),
            database_level: ImsDbLevel::Current,
            pcbs: vec![ImsPcbMetadata::Database(ImsDatabasePcbMetadata {
                name: "BINDPCB".into(),
                database: "BINDDB".into(),
                database_version: Some(1),
                secondary_index: None,
                processing_options: "AP".into(),
                sensitive_segments: vec![ImsSensitiveSegmentMetadata {
                    name: "ROOT".into(),
                    parent: None,
                    processing_options: None,
                }],
            })],
        }],
    }
}

fn invocation(class: ServiceClass) -> Invocation {
    let limits = InvocationLimits::default();
    Invocation::new(
        RequestId::new("participant-request", limits).unwrap(),
        ExecutionId::new("participant-execution", limits).unwrap(),
        RunUnitId::new("participant-run", limits).unwrap(),
        None,
        Selector::new("ims:binding", limits).unwrap(),
        ArtifactRef::new("ims:binding", limits).unwrap(),
        Principal::new(
            PrincipalId::new("TESTUSER", limits).unwrap(),
            ["host.ims.read", "host.ims.write"]
                .into_iter()
                .map(|id| CapabilityId::new(id, limits).unwrap())
                .collect(),
            limits,
        )
        .unwrap(),
        class,
        0,
        100,
        TraceId::new("participant-trace", limits).unwrap(),
        IdempotencyKey::new("participant-invocation", limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .unwrap()
}

fn request(op: ImsOperation, sequence: u64, data: &[u8]) -> ImsRequest {
    ImsRequest {
        operation: op,
        psb: match op {
            ImsOperation::Schedule => Some("BINDPSB".into()),
            ImsOperation::Unload => Some("BINDDB".into()),
            _ => None,
        },
        pcb: 1,
        segments: if op == ImsOperation::Insert {
            vec!["ROOT".into()]
        } else {
            vec![]
        },
        data: data.to_vec(),
        qualifiers: vec![],
        checkpoint_id: None,
        max_segments: 64,
        mutation: op.is_mutating().then(|| Mutation {
            sequence,
            idempotency_key: IdempotencyKey::new(
                format!("participant-{sequence}"),
                InvocationLimits::default(),
            )
            .unwrap(),
            transaction: Some("LOCAL-IMS".into()),
        }),
        system: None,
        q_class: None,
    }
}

fn invoke(
    service: &Arc<ImsService>,
    invocation: &Invocation,
    request: &ImsRequest,
) -> Result<ImsResult, HostProblem> {
    // Exercise the public host provider, not a private handler or a generated expected result.
    let providers = ims_providers(service.clone(), InvocationLimits::default());
    let effect = EffectRequest {
        run_unit: invocation.run_unit_id.clone(),
        sequence: request.mutation.as_ref().map_or(99, |m| m.sequence),
        idempotency_key: request.mutation.as_ref().map(|m| m.idempotency_key.clone()),
        request: HostRequest::Ims(request.clone()),
        deadline_tick: invocation.deadline_tick,
    };
    let result = providers[usize::from(request.operation.is_mutating())].invoke(invocation, effect);
    match result.outcome? {
        HostResult::Ims(result) => Ok(result),
        _ => panic!("unexpected host result"),
    }
}

fn success(
    service: &Arc<ImsService>,
    invocation: &Invocation,
    op: ImsOperation,
    seq: u64,
    data: &[u8],
) -> ImsResult {
    let result = invoke(service, invocation, &request(op, seq, data)).unwrap();
    assert_eq!(result.status, "  ");
    result
}

fn data(service: &Arc<ImsService>, invocation: &Invocation) -> Vec<Vec<u8>> {
    success(service, invocation, ImsOperation::Unload, 99, b"")
        .segments
        .into_iter()
        .map(|s| s.data)
        .collect()
}

fn rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    store.list_provider_state_prefix("ims-", 128).unwrap()
}

#[test]
fn memory_local_commit_rollback_replay_and_batch_limit() {
    for class in [ServiceClass::Interactive, ServiceClass::Batch] {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let auth = Arc::new(Authorization::default());
        let service =
            ImsService::open_authorized(store.clone(), Default::default(), auth.clone()).unwrap();
        service.install_metadata(catalog()).unwrap();
        let invocation = invocation(class);
        success(&service, &invocation, ImsOperation::Schedule, 1, b"");
        let insert = request(ImsOperation::Insert, 2, b"R1A");
        let result = invoke(&service, &invocation, &insert).unwrap();
        assert_eq!(result.affected_segments, 1);
        let before = rows(store.as_ref());
        assert_eq!(invoke(&service, &invocation, &insert), Ok(result));
        assert_eq!(rows(store.as_ref()), before);
        let mut different = insert.clone();
        different.data = b"R2B".to_vec();
        assert_eq!(
            invoke(&service, &invocation, &different),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(rows(store.as_ref()), before);
        success(&service, &invocation, ImsOperation::Rollback, 3, b"");
        assert!(data(&service, &invocation).is_empty());
        success(&service, &invocation, ImsOperation::Insert, 4, b"R3C");
        success(&service, &invocation, ImsOperation::Commit, 5, b"");
        success(&service, &invocation, ImsOperation::Rollback, 6, b"");
        assert!(data(&service, &invocation).contains(&b"R3C".to_vec()));
        assert!(
            auth.seen
                .lock()
                .unwrap()
                .iter()
                .all(|(id, _)| id == invocation.principal.id())
        );
    }
}

#[test]
fn authorization_and_malformed_failures_preserve_all_rows() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let auth = Arc::new(Authorization::default());
    let service =
        ImsService::open_authorized(store.clone(), Default::default(), auth.clone()).unwrap();
    service.install_metadata(catalog()).unwrap();
    let invocation = invocation(ServiceClass::Interactive);
    success(&service, &invocation, ImsOperation::Schedule, 1, b"");
    let before = rows(store.as_ref());
    let insert = request(ImsOperation::Insert, 2, b"R1A");
    auth.deny.store(true, Ordering::SeqCst);
    assert_eq!(
        invoke(&service, &invocation, &insert),
        Err(HostProblem::Unauthorized)
    );
    assert_eq!(rows(store.as_ref()), before);
    auth.deny.store(false, Ordering::SeqCst);
    auth.fail.store(true, Ordering::SeqCst);
    assert_eq!(
        invoke(&service, &invocation, &insert),
        Err(HostProblem::InfrastructureFailure)
    );
    assert_eq!(rows(store.as_ref()), before);
    auth.fail.store(false, Ordering::SeqCst);
    let mut malformed = insert.clone();
    malformed.mutation = None;
    assert_eq!(
        invoke(&service, &invocation, &malformed),
        Err(HostProblem::MissingIdempotency)
    );
    assert_eq!(rows(store.as_ref()), before);
}

#[test]
fn sqlite_process_restart_preserves_commit_undo_and_unknown_replay() {
    let root = std::env::temp_dir().join(format!("ims-participant-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let url = format!("sqlite://{}/state.sqlite?mode=rwc", root.display());
    for phase in ["seed", "rollback", "unknown", "resolve"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "sqlite_child_phase", "--nocapture"])
            .env("IMS_PARTICIPANT_TEST_URL", &url)
            .env("IMS_PARTICIPANT_TEST_PHASE", phase)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{phase}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    // Remove only this explicitly constructed test directory after all children exit.
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn sqlite_child_phase() {
    let Ok(url) = std::env::var("IMS_PARTICIPANT_TEST_URL") else {
        return;
    };
    let phase = std::env::var("IMS_PARTICIPANT_TEST_PHASE").unwrap();
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    let clock = Arc::new(ReplayClock(AtomicBool::new(false)));
    let service = ImsService::open_authorized_with_replay_clock(
        store.clone(),
        Default::default(),
        Arc::new(Authorization::default()),
        clock.clone(),
    )
    .unwrap();
    let invocation = invocation(ServiceClass::Interactive);
    match phase.as_str() {
        "seed" => {
            service.install_metadata(catalog()).unwrap();
            success(&service, &invocation, ImsOperation::Schedule, 1, b"");
            success(&service, &invocation, ImsOperation::Insert, 2, b"R1A");
            success(&service, &invocation, ImsOperation::Commit, 3, b"");
            success(&service, &invocation, ImsOperation::Insert, 4, b"R2B");
            assert_eq!(
                data(&service, &invocation),
                vec![b"R1A".to_vec(), b"R2B".to_vec()]
            );
        }
        "rollback" => {
            assert_eq!(data(&service, &invocation).len(), 2);
            let before = rows(store.as_ref());
            success(&service, &invocation, ImsOperation::Insert, 2, b"R1A");
            assert_eq!(rows(store.as_ref()), before);
            success(&service, &invocation, ImsOperation::Rollback, 5, b"");
            assert_eq!(data(&service, &invocation), vec![b"R1A".to_vec()]);
            // An old insert receipt must not reintroduce data already rolled back.
            success(&service, &invocation, ImsOperation::Insert, 4, b"R2B");
            assert_eq!(data(&service, &invocation), vec![b"R1A".to_vec()]);
        }
        "unknown" => {
            clock.0.store(true, Ordering::SeqCst);
            let insert = request(ImsOperation::Insert, 6, b"R3C");
            assert_eq!(
                invoke(&service, &invocation, &insert),
                Err(HostProblem::UnknownOutcome)
            );
            let before = rows(store.as_ref());
            assert_eq!(
                invoke(&service, &invocation, &insert),
                Err(HostProblem::UnknownOutcome)
            );
            assert_eq!(rows(store.as_ref()), before);
            assert_eq!(
                data(&service, &invocation),
                vec![b"R1A".to_vec(), b"R3C".to_vec()]
            );
        }
        "resolve" => {
            assert_eq!(data(&service, &invocation).len(), 2);
            let database = store
                .get_provider_state("ims-v1-generic-database", "BINDDB")
                .unwrap();
            success(&service, &invocation, ImsOperation::Insert, 6, b"R3C");
            assert_eq!(
                store
                    .get_provider_state("ims-v1-generic-database", "BINDDB")
                    .unwrap(),
                database
            );
            success(&service, &invocation, ImsOperation::Rollback, 7, b"");
            assert_eq!(data(&service, &invocation), vec![b"R1A".to_vec()]);
        }
        _ => panic!("unknown phase"),
    }
}

#[test]
fn descriptor_preparation_keeps_integration_pending() {
    let descriptor = transaction_participant_contract_v1()
        .participant("ims")
        .unwrap();
    assert_eq!(descriptor.status, ParticipantStatus::Pending);
    assert!(descriptor.capabilities.is_none());
    assert_eq!(
        descriptor.preparation_contract_test,
        Some("crates/providers/mainframe-env-ims/tests/participant_contract.rs")
    );
    for gap in [
        "INT-1601.fencing",
        "INT-1601.security-audit",
        "INT-1601.deadline-cancellation",
    ] {
        assert!(descriptor.blocked_obligations.contains(&gap));
    }
}
