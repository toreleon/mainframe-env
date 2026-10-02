//! Independent, zero-credit expectations through selected public host dispatch.
use mainframe_env_execution_api::{
    ArtifactRef, CapabilityId, ExecutionId, IdempotencyKey, Invocation, InvocationLimits,
    Principal, PrincipalId, RequestId, ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
};
use mainframe_env_host_api::{
    EffectRequest, EnterpriseAuthorizer, EnterpriseResource, HostLimits, HostProblem, HostRequest,
    HostResult, ImsCallSyntax, ImsExecutionContext, ImsRecoveryCall, ImsRecoveryRequest,
    ImsRecoveryResult, Mutation, RegistrySnapshot, ScopedHostService, canonical_request_digest,
};
use mainframe_env_ims::recovery::{RecoveryLimits, RecoverySession};
use mainframe_env_ims::*;
use mainframe_env_store::{MemoryStore, SqliteStateStore};
use mainframe_env_store_api::{
    AuditSink, EffectDigestFormat, EffectIntentMetadata, EffectRecord, EffectState,
    IdempotencyStore, ProviderStateMutation, ProviderStateRecord, ProviderStateStore,
    ProviderStateWrite, StoreError,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

trait TestStore: IdempotencyStore + ProviderStateStore {}
impl<T: IdempotencyStore + ProviderStateStore> TestStore for T {}

const PACKAGE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct Allow;
impl EnterpriseAuthorizer for Allow {
    fn authorize(&self, _: &PrincipalId, _: &EnterpriseResource) -> Result<(), HostProblem> {
        Ok(())
    }
}

fn catalog() -> ImsMetadataCatalog {
    ImsMetadataCatalog {
        schema_version: IMS_METADATA_SCHEMA_V1.into(),
        databases: vec![ImsDatabaseMetadata {
            name: "LOGDB".into(),
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
            name: "LOGPSB".into(),
            database_level: ImsDbLevel::Current,
            pcbs: vec![ImsPcbMetadata::Database(ImsDatabasePcbMetadata {
                name: "DBPCB".into(),
                database: "LOGDB".into(),
                database_version: Some(1),
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

fn invocation() -> Invocation {
    let ids = InvocationLimits::default();
    Invocation::new(
        RequestId::new("log-request", ids).unwrap(),
        ExecutionId::new("log-execution", ids).unwrap(),
        RunUnitId::new("log-run", ids).unwrap(),
        None,
        Selector::new("ims:log", ids).unwrap(),
        ArtifactRef::new("ims:log", ids).unwrap(),
        Principal::new(
            PrincipalId::new("LOGUSER", ids).unwrap(),
            [CapabilityId::new("host.ims.write", ids).unwrap()]
                .into_iter()
                .collect(),
            ids,
        )
        .unwrap(),
        ServiceClass::Batch,
        0,
        100,
        TraceId::new("log-trace", ids).unwrap(),
        IdempotencyKey::new("log-invocation", ids).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        ids,
    )
    .unwrap()
}

fn request(sequence: u64) -> ImsRecoveryRequest {
    ImsRecoveryRequest {
        application: "LOGAPP".into(),
        package_identity: PACKAGE.into(),
        psb: "LOGPSB".into(),
        database: "LOGDB".into(),
        context: ImsExecutionContext::DbBatch,
        syntax: ImsCallSyntax::Call,
        call: ImsRecoveryCall::Log {
            code: 0xa0,
            data: vec![0, 0xff, b'\n'],
        },
        mutation: Mutation {
            sequence,
            idempotency_key: IdempotencyKey::new(
                format!("log-{sequence}"),
                InvocationLimits::default(),
            )
            .unwrap(),
            transaction: None,
        },
    }
}

fn intent(store: &dyn IdempotencyStore, invocation: &Invocation, request: &ImsRecoveryRequest) {
    store
        .record_intent(EffectRecord {
            execution_id: invocation.execution_id.clone(),
            run_unit_id: invocation.run_unit_id.clone(),
            sequence: request.mutation.sequence,
            key: request.mutation.idempotency_key.clone(),
            digest_format: EffectDigestFormat::CanonicalHostV1,
            request_digest: canonical_request_digest(&HostRequest::ImsRecovery(request.clone()))
                .unwrap(),
            intent: EffectIntentMetadata {
                owner: invocation.execution_id.clone(),
                attempt: 1,
                capability: Some(
                    CapabilityId::new("host.ims.write", InvocationLimits::default()).unwrap(),
                ),
                audit_resource: None,
                audit_invocation_key: None,
                created_tick: 1,
                recovery_after_tick: 100,
                epoch: request.mutation.sequence,
                recovery_lease: None,
            },
            state: EffectState::Intent,
            result_digest: None,
            resolved_tick: None,
        })
        .unwrap();
}

fn open(store: Arc<dyn ProviderStateStore>) -> Arc<ImsService> {
    let service =
        ImsService::open_authorized(store, ImsLimits::default(), Arc::new(Allow)).unwrap();
    service.install_metadata(catalog()).unwrap();
    service
        .publish_metadata_generation("LOGAPP", 1, PACKAGE, Some(&catalog()))
        .unwrap();
    service
}

fn log_count(store: &dyn ProviderStateStore) -> usize {
    let rows = store
        .list_provider_state("ims-recovery-v1-session", 64)
        .unwrap();
    rows.iter()
        .map(|row| {
            RecoverySession::load(store, &row.key, RecoveryLimits::default())
                .unwrap()
                .log_count()
        })
        .sum()
}

fn dispatch(
    service: Arc<ImsService>,
    effects: Arc<dyn TestStore>,
    invocation: &Invocation,
    request: &ImsRecoveryRequest,
) -> Result<ImsRecoveryResult, HostProblem> {
    let ids = InvocationLimits::default();
    let registry = RegistrySnapshot::new(
        1,
        ims_providers_with_recovery(service, effects.clone(), ids),
        ids,
    )
    .unwrap();
    let host = ScopedHostService::new(Arc::new(registry), HostLimits::default());
    let result = host
        .invoke(
            invocation,
            2,
            false,
            EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence: request.mutation.sequence,
                idempotency_key: Some(request.mutation.idempotency_key.clone()),
                request: HostRequest::ImsRecovery(request.clone()),
                deadline_tick: invocation.deadline_tick,
            },
        )
        .persist_with(|audit| {
            effects
                .record_audit(audit)
                .map_err(|_| HostProblem::InfrastructureFailure)
        });
    match result.outcome? {
        HostResult::ImsRecovery(result) => Ok(result),
        _ => panic!("expected owned IMS recovery response"),
    }
}

fn backends(name: &str, case: impl Fn(Arc<dyn TestStore>)) {
    case(Arc::new(MemoryStore::new(Default::default())));
    let path = std::env::temp_dir().join(format!("ims-log-{name}-{}.sqlite", std::process::id()));
    let url = format!("sqlite:{}?mode=rwc", path.display());
    case(Arc::new(
        SqliteStateStore::open(&url, 64 * 1024 * 1024, 4096).unwrap(),
    ));
    std::fs::remove_file(path).unwrap();
}

fn recovery_rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    store
        .list_provider_state("ims-recovery-v1-session", 64)
        .unwrap()
}

#[test]
fn public_log_validates_binding_identity_context_and_canonical_intent_on_both_backends() {
    backends("validation", |store| {
        let service = open(store.clone());
        let invocation = invocation();
        assert_eq!(
            dispatch(service.clone(), store.clone(), &invocation, &request(1)),
            Err(HostProblem::MissingIdempotency)
        );
        for (index, expected) in [
            HostProblem::Malformed,
            HostProblem::ResourceExhausted,
            HostProblem::NotFound,
            HostProblem::IdempotencyConflict,
            HostProblem::NotFound,
            HostProblem::Unsupported,
            HostProblem::Unsupported,
        ]
        .into_iter()
        .enumerate()
        {
            let mut r = request(index as u64 + 2);
            match index {
                0 => {
                    r.call = ImsRecoveryCall::Log {
                        code: 0x9f,
                        data: vec![],
                    }
                }
                1 => {
                    r.call = ImsRecoveryCall::Log {
                        code: 0xa0,
                        data: vec![0; 32 * 1024 + 1],
                    }
                }
                2 => r.application = "MISSING".into(),
                3 => r.package_identity = format!("sha256:{}", "b".repeat(64)),
                4 => r.psb = "MISSING".into(),
                5 => r.syntax = ImsCallSyntax::Command,
                _ => r.mutation.transaction = Some("EXTERNAL".into()),
            }
            assert_eq!(
                dispatch(service.clone(), store.clone(), &invocation, &r),
                Err(expected)
            );
            assert!(recovery_rows(&*store).is_empty());
        }
        let r = request(20);
        intent(&*store, &invocation, &r);
        let mut changed = r.clone();
        changed.call = ImsRecoveryCall::Log {
            code: 0xff,
            data: vec![],
        };
        assert_eq!(
            dispatch(service.clone(), store.clone(), &invocation, &changed),
            Err(HostProblem::IdempotencyConflict)
        );
        let mut wrong_owner = invocation.clone();
        wrong_owner.execution_id =
            ExecutionId::new("another-execution", InvocationLimits::default()).unwrap();
        assert_eq!(
            dispatch(service.clone(), store.clone(), &wrong_owner, &r),
            Err(HostProblem::IdempotencyConflict)
        );
        wrong_owner = invocation.clone();
        wrong_owner.service_class = ServiceClass::Interactive;
        assert_eq!(
            dispatch(service.clone(), store.clone(), &wrong_owner, &r),
            Err(HostProblem::Unsupported)
        );
        wrong_owner = invocation.clone();
        wrong_owner.deadline_tick = 2;
        assert_eq!(
            dispatch(service.clone(), store.clone(), &wrong_owner, &r),
            Err(HostProblem::TimedOut)
        );
        assert!(recovery_rows(&*store).is_empty());
        assert!(
            !store
                .audit_records(&invocation.execution_id, 0, 32)
                .unwrap()
                .is_empty()
        );
    });
}

struct Deny(HostProblem);
impl EnterpriseAuthorizer for Deny {
    fn authorize(&self, _: &PrincipalId, _: &EnterpriseResource) -> Result<(), HostProblem> {
        Err(self.0.clone())
    }
}

#[test]
fn log_saf_and_missing_authorizer_fail_before_recovery_observation_or_mutation() {
    backends("saf", |store| {
        let initial = open(store.clone());
        drop(initial);
        let invocation = invocation();
        let r = request(1);
        intent(&*store, &invocation, &r);
        for problem in [
            HostProblem::Unauthorized,
            HostProblem::InfrastructureFailure,
        ] {
            let service = ImsService::open_authorized(
                store.clone(),
                ImsLimits::default(),
                Arc::new(Deny(problem.clone())),
            )
            .unwrap();
            assert_eq!(
                dispatch(service, store.clone(), &invocation, &r),
                Err(problem)
            );
            assert!(recovery_rows(&*store).is_empty());
        }
        let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        assert_eq!(
            dispatch(service, store.clone(), &invocation, &r),
            Err(HostProblem::Unauthorized)
        );
        let audits = store
            .audit_records(&invocation.execution_id, 0, 16)
            .unwrap();
        assert!(
            audits
                .iter()
                .any(|a| a.decision == mainframe_env_execution_api::AuditDecision::Deny)
        );
    });
}

fn database_request(
    operation: mainframe_env_host_api::ImsOperation,
    sequence: u64,
    data: &[u8],
) -> mainframe_env_host_api::ImsRequest {
    use mainframe_env_host_api::*;
    ImsRequest {
        operation,
        psb: (operation == ImsOperation::Schedule).then(|| "LOGPSB".into()),
        pcb: 1,
        segments: if matches!(operation, ImsOperation::Insert | ImsOperation::GetUnique) {
            vec!["ROOT".into()]
        } else {
            vec![]
        },
        data: data.to_vec(),
        qualifiers: vec![],
        checkpoint_id: None,
        max_segments: 16,
        mutation: operation.is_mutating().then(|| Mutation {
            sequence,
            idempotency_key: IdempotencyKey::new(
                format!("db-{sequence}"),
                InvocationLimits::default(),
            )
            .unwrap(),
            transaction: None,
        }),
        system: None,
        q_class: None,
    }
}

#[test]
fn log_preserves_real_gu_position_and_survives_local_and_package_rollback() {
    use mainframe_env_host_api::ImsOperation;
    backends("position", |store| {
        let service = open(store.clone());
        let invocation = invocation();
        for (op, sequence, data) in [
            (ImsOperation::Schedule, 10, &b""[..]),
            (ImsOperation::Insert, 11, &b"01A"[..]),
            (ImsOperation::Insert, 12, &b"02B"[..]),
            (ImsOperation::GetUnique, 13, &b""[..]),
        ] {
            assert_eq!(
                service
                    .execute(&invocation, &database_request(op, sequence, data))
                    .unwrap()
                    .status,
                "  "
            );
        }
        let before = [
            "ims-v1-generic-database",
            "ims-v1-session-index",
            "ims-v1-generic-unit-of-work",
        ]
        .map(|ns| store.list_provider_state(ns, 64).unwrap());
        let r = request(1);
        intent(&*store, &invocation, &r);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &invocation, &r),
            Ok(ImsRecoveryResult::Logged {
                status: "  ".into(),
                sequence: 1
            })
        );
        assert_eq!(
            [
                "ims-v1-generic-database",
                "ims-v1-session-index",
                "ims-v1-generic-unit-of-work"
            ]
            .map(|ns| store.list_provider_state(ns, 64).unwrap()),
            before
        );
        let next = service
            .execute(
                &invocation,
                &database_request(ImsOperation::GetNext, 14, &[]),
            )
            .unwrap();
        assert_eq!(next.status, "  ");
        assert_eq!(next.segments[0].data, b"02B");
        assert_eq!(
            service
                .execute(
                    &invocation,
                    &database_request(ImsOperation::Rollback, 15, &[])
                )
                .unwrap()
                .status,
            "  "
        );
        let logs = recovery_rows(&*store);
        let rows: serde_json::Value = serde_json::from_slice(&logs[0].payload).unwrap();
        assert_eq!(rows["logs"][0]["code"], 160);
        assert_eq!(rows["logs"][0]["data"], serde_json::json!([0, 255, 10]));
        service
            .publish_metadata_generation(
                "LOGAPP",
                2,
                &format!("sha256:{}", "b".repeat(64)),
                Some(&catalog()),
            )
            .unwrap();
        assert_eq!(
            dispatch(service.clone(), store.clone(), &invocation, &r),
            Err(HostProblem::IdempotencyConflict)
        );
        service
            .publish_metadata_generation("LOGAPP", 1, PACKAGE, Some(&catalog()))
            .unwrap();
        assert_eq!(
            dispatch(service, store.clone(), &invocation, &r),
            Ok(ImsRecoveryResult::Logged {
                status: "  ".into(),
                sequence: 1
            })
        );
        assert_eq!(recovery_rows(&*store), logs);
    });
}

/// Fault injection delegates every real row operation to the selected backend.
struct FaultRows {
    inner: Arc<dyn TestStore>,
    mode: AtomicU8,
}
impl AuditSink for FaultRows {
    fn record_audit(
        &self,
        audit: mainframe_env_execution_api::AuditRecord,
    ) -> Result<(), StoreError> {
        self.inner.record_audit(audit)
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
impl ProviderStateStore for FaultRows {
    fn get_provider_state(
        &self,
        ns: &str,
        key: &str,
    ) -> Result<Option<ProviderStateRecord>, StoreError> {
        self.inner.get_provider_state(ns, key)
    }
    fn list_provider_state(
        &self,
        ns: &str,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        self.inner.list_provider_state(ns, max)
    }
    fn put_provider_state(
        &self,
        r: ProviderStateRecord,
        version: Option<u64>,
    ) -> Result<(), StoreError> {
        self.inner.put_provider_state(r, version)
    }
    fn delete_provider_state(&self, ns: &str, key: &str, version: u64) -> Result<(), StoreError> {
        self.inner.delete_provider_state(ns, key, version)
    }
    fn move_provider_state(
        &self,
        r: ProviderStateRecord,
        key: &str,
        version: u64,
    ) -> Result<(), StoreError> {
        self.inner.move_provider_state(r, key, version)
    }
    fn put_provider_states_atomic(
        &self,
        writes: Vec<ProviderStateWrite>,
    ) -> Result<(), StoreError> {
        self.inner.put_provider_states_atomic(writes)
    }
    fn mutate_provider_states_atomic(
        &self,
        mutations: Vec<ProviderStateMutation>,
    ) -> Result<(), StoreError> {
        match self.mode.swap(0, Ordering::SeqCst) {
            1 => return Err(StoreError::CapacityExceeded),
            2 => {
                self.inner.mutate_provider_states_atomic(mutations)?;
                return Err(StoreError::Infrastructure("lost acknowledgement".into()));
            }
            3 => {
                let mut row = self
                    .inner
                    .get_provider_state("ims-v1-metadata-selection", "LOGAPP")?
                    .unwrap();
                let old = row.version;
                row.version += 1;
                self.inner.put_provider_state(row, Some(old))?;
            }
            _ => {}
        }
        self.inner.mutate_provider_states_atomic(mutations)
    }
}

#[test]
fn log_atomic_capacity_cas_and_lost_ack_are_truthful_on_both_backends() {
    backends("fault", |store| {
        let faults = Arc::new(FaultRows {
            inner: store.clone(),
            mode: AtomicU8::new(0),
        });
        let service = open(faults.clone());
        let invocation = invocation();
        for (mode, sequence, problem) in [
            (1, 1, HostProblem::ResourceExhausted),
            (3, 2, HostProblem::IdempotencyConflict),
        ] {
            let r = request(sequence);
            intent(&*store, &invocation, &r);
            let databases = store
                .list_provider_state("ims-v1-generic-database", 64)
                .unwrap();
            faults.mode.store(mode, Ordering::SeqCst);
            assert_eq!(
                dispatch(service.clone(), store.clone(), &invocation, &r),
                Err(problem)
            );
            assert!(recovery_rows(&*store).is_empty());
            assert_eq!(
                store
                    .list_provider_state("ims-v1-generic-database", 64)
                    .unwrap(),
                databases
            );
        }
        let r = request(3);
        intent(&*store, &invocation, &r);
        faults.mode.store(2, Ordering::SeqCst);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &invocation, &r),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(log_count(&*store), 1);
        // A canonical unknown receipt stops this route before any redispatch.
        let mut effect = store.effect(&r.mutation.idempotency_key).unwrap().unwrap();
        effect.state = EffectState::UnknownOutcome;
        effect.result_digest = Some(
            mainframe_env_host_api::canonical_result_digest(&Err(HostProblem::UnknownOutcome))
                .unwrap(),
        );
        effect.resolved_tick = None;
        store
            .record_result(&r.mutation.idempotency_key, effect)
            .unwrap();
        let rows = recovery_rows(&*store);
        assert_eq!(
            dispatch(service, store.clone(), &invocation, &r),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(recovery_rows(&*store), rows);
    });
}

#[test]
fn sqlite_log_child_process_restart_retains_exact_replay_and_order() {
    let path = std::env::temp_dir().join(format!("ims-log-process-{}.sqlite", std::process::id()));
    for phase in ["append", "reopen"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "sqlite_log_process_worker", "--nocapture"])
            .env("IMS_LOG_TEST_PATH", &path)
            .env("IMS_LOG_TEST_PHASE", phase)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn sqlite_log_process_worker() {
    let Ok(path) = std::env::var("IMS_LOG_TEST_PATH") else {
        return;
    };
    let store = Arc::new(
        SqliteStateStore::open(&format!("sqlite:{path}?mode=rwc"), 64 * 1024 * 1024, 4096).unwrap(),
    );
    let service = open(store.clone());
    let invocation = invocation();
    if std::env::var("IMS_LOG_TEST_PHASE").unwrap() == "append" {
        for sequence in [1, 2] {
            let r = request(sequence);
            intent(&*store, &invocation, &r);
            assert_eq!(
                dispatch(service.clone(), store.clone(), &invocation, &r),
                Ok(ImsRecoveryResult::Logged {
                    status: "  ".into(),
                    sequence
                })
            );
        }
    } else {
        let before = recovery_rows(&*store);
        assert_eq!(log_count(&*store), 2);
        assert_eq!(
            dispatch(service, store.clone(), &invocation, &request(1)),
            Ok(ImsRecoveryResult::Logged {
                status: "  ".into(),
                sequence: 1
            })
        );
        assert_eq!(recovery_rows(&*store), before);
    }
}

struct DenyDatabase;
impl EnterpriseAuthorizer for DenyDatabase {
    fn authorize(&self, _: &PrincipalId, resource: &EnterpriseResource) -> Result<(), HostProblem> {
        if resource.class == mainframe_env_host_api::EnterpriseResourceClass::ImsDatabase {
            assert_eq!(resource.name.as_str(), "LOGDB");
            assert_eq!(
                resource.intent,
                mainframe_env_host_api::AccessIntent::Update
            );
            Err(HostProblem::Unauthorized)
        } else {
            assert_eq!(
                resource.class,
                mainframe_env_host_api::EnterpriseResourceClass::ImsPsb
            );
            Ok(())
        }
    }
}

#[test]
fn database_saf_and_claimed_effect_stop_log_dispatch_before_mutation() {
    backends("lease", |store| {
        let initial = open(store.clone());
        drop(initial);
        let invocation = invocation();
        let r = request(1);
        intent(&*store, &invocation, &r);
        let denied = ImsService::open_authorized(
            store.clone(),
            ImsLimits::default(),
            Arc::new(DenyDatabase),
        )
        .unwrap();
        assert_eq!(
            dispatch(denied, store.clone(), &invocation, &r),
            Err(HostProblem::Unauthorized)
        );
        assert!(recovery_rows(&*store).is_empty());
        store
            .claim_stale_intent(
                &r.mutation.idempotency_key,
                1,
                "recovery-worker",
                101,
                1,
                10,
            )
            .unwrap();
        let service = open(store.clone());
        assert_eq!(
            dispatch(service, store.clone(), &invocation, &r),
            Err(HostProblem::UnknownOutcome)
        );
        assert!(recovery_rows(&*store).is_empty());
    });
}

#[test]
fn corrupt_or_future_recovery_rows_fail_closed_on_both_backends() {
    backends("corruption", |store| {
        let service = open(store.clone());
        let invocation = invocation();
        let r = request(1);
        intent(&*store, &invocation, &r);
        dispatch(service.clone(), store.clone(), &invocation, &r).unwrap();
        let original = recovery_rows(&*store).remove(0);
        for unknown_version in [false, true] {
            let mut row = original.clone();
            let current = store
                .get_provider_state(&row.namespace, &row.key)
                .unwrap()
                .unwrap();
            row.version = current.version + 1;
            let mut body: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
            if unknown_version {
                body["schema_version"] = "mainframe-env.ims-recovery-session@999".into();
            } else {
                body["logs"][0]["data"] = serde_json::json!([7]);
            }
            row.payload = serde_json::to_vec(&body).unwrap();
            store
                .put_provider_state(row.clone(), Some(current.version))
                .unwrap();
            assert_eq!(
                dispatch(service.clone(), store.clone(), &invocation, &r),
                Err(HostProblem::ProviderFailure)
            );
            assert_eq!(recovery_rows(&*store), vec![row]);
        }
    });
}

#[test]
fn selected_memory_log_dispatch_appends_once_and_preserves_database() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let service = open(store.clone());
    let before = store
        .list_provider_state("ims-v1-generic-database", 64)
        .unwrap();
    let invocation = invocation();
    let request = request(1);
    intent(&*store, &invocation, &request);
    let expected = ImsRecoveryResult::Logged {
        status: "  ".into(),
        sequence: 1,
    };
    assert_eq!(
        dispatch(service.clone(), store.clone(), &invocation, &request),
        Ok(expected.clone())
    );
    assert_eq!(
        dispatch(service, store.clone(), &invocation, &request),
        Ok(expected)
    );
    assert_eq!(
        store
            .list_provider_state("ims-v1-generic-database", 64)
            .unwrap(),
        before
    );
    assert_eq!(log_count(&*store), 1);
}

#[test]
fn selected_sqlite_log_reopens_and_rejects_unsupported_context_without_mutation() {
    let path = std::env::temp_dir().join(format!("ims-log-route-{}.sqlite", std::process::id()));
    let url = format!("sqlite:{}?mode=rwc", path.display());
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 4096).unwrap());
    let service = open(store.clone());
    let invocation = invocation();
    let mut request = request(1);
    intent(&*store, &invocation, &request);
    assert_eq!(
        dispatch(service, store.clone(), &invocation, &request),
        Ok(ImsRecoveryResult::Logged {
            status: "  ".into(),
            sequence: 1
        })
    );
    drop(store);
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 4096).unwrap());
    let service = open(store.clone());
    request.context = ImsExecutionContext::DbDc;
    assert_eq!(
        dispatch(service, store.clone(), &invocation, &request),
        Err(HostProblem::Unsupported)
    );
    assert_eq!(log_count(&*store), 1);
    drop(store);
    std::fs::remove_file(path).unwrap();
}
