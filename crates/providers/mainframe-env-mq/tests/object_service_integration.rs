use mainframe_env_execution_api::{
    ArtifactRef, BoundedPayload, CapabilityId, ExecutionId, IdempotencyKey, Invocation,
    InvocationLimits, Principal, PrincipalId, RequestId, ResourceLimits, RunUnitId, Selector,
    ServiceClass, TraceId,
};
use mainframe_env_host_api::{
    EnterpriseAuthorizer, EnterpriseResource, HostProblem, MqOperation, MqRequest, Mutation,
};
use mainframe_env_mq::{
    MqAliasTarget, MqDynamicQueueKind, MqDynamicQueuePattern, MqLifecycleOwner, MqLimits,
    MqLocalQueueUsage, MqObjectCatalog, MqObjectDefinition, MqObjectLimits, MqObjectName,
    MqQueueDefinition, MqQueueManagerDefinition, MqService,
};
use mainframe_env_store::{MemoryStore, SqliteStateStore};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn name(value: &str) -> MqObjectName {
    MqObjectName::new(value).unwrap()
}

fn catalog() -> MqObjectCatalog {
    MqObjectCatalog::new(
        MqQueueManagerDefinition {
            name: name("QM.LOCAL"),
            default_transmission_queue: Some(name("XMIT.Q")),
        },
        vec![
            MqObjectDefinition::LocalQueue {
                name: name("queue"),
                usage: MqLocalQueueUsage::Normal,
                trigger_process: Some(name("PROC")),
            },
            MqObjectDefinition::LocalQueue {
                name: name("XMIT.Q"),
                usage: MqLocalQueueUsage::Transmission,
                trigger_process: None,
            },
            MqObjectDefinition::AliasQueue {
                name: name("alias"),
                target: MqAliasTarget::Queue(name("queue")),
            },
            MqObjectDefinition::RemoteQueue {
                name: name("REMOTE.Q"),
                remote_queue: Some(name("OTHER.Q")),
                remote_queue_manager: name("OTHER.QM"),
                transmission_queue: None,
            },
            MqObjectDefinition::ModelQueue {
                name: name("MODEL.Q"),
                definition_type: MqDynamicQueueKind::Permanent,
                trigger_process: None,
            },
            MqObjectDefinition::Topic {
                name: name("TOPIC.A"),
            },
            MqObjectDefinition::Process { name: name("PROC") },
        ],
        MqObjectLimits::default(),
    )
    .unwrap()
}

fn invocation(run: &str) -> Invocation {
    let limits = InvocationLimits::default();
    Invocation::new(
        RequestId::new(format!("request-{run}"), limits).unwrap(),
        ExecutionId::new(format!("execution-{run}"), limits).unwrap(),
        RunUnitId::new(run, limits).unwrap(),
        None,
        Selector::new("mq:object-integration", limits).unwrap(),
        ArtifactRef::new("mq:object-integration", limits).unwrap(),
        Principal::new(
            PrincipalId::new("IBMUSER", limits).unwrap(),
            BTreeSet::from([CapabilityId::new("host.mq.write", limits).unwrap()]),
            limits,
        )
        .unwrap(),
        ServiceClass::Interactive,
        0,
        100,
        TraceId::new(format!("trace-{run}"), limits).unwrap(),
        IdempotencyKey::new(format!("invocation-{run}"), limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::from([(
            "mq.host-context".into(),
            BoundedPayload::new(
                "mainframe-env.mq.host-context@1",
                b"other-bindings|queue-manager".to_vec(),
                limits,
            )
            .unwrap(),
        )]),
        limits,
    )
    .unwrap()
}

fn request(operation: MqOperation, queue: &str, sequence: u64) -> MqRequest {
    MqRequest {
        operation,
        queue: Some(queue.into()),
        handle: None,
        options: 0,
        message: Vec::new(),
        message_id: None,
        correlation_id: None,
        wait_ticks: 0,
        max_message_bytes: 1024,
        mutation: Some(Mutation {
            sequence,
            idempotency_key: IdempotencyKey::new(
                format!("object-effect-{sequence}"),
                InvocationLimits::default(),
            )
            .unwrap(),
            transaction: Some("MQ-OBJECT-TEST".into()),
        }),
    }
}

#[derive(Default)]
struct Deny;

impl EnterpriseAuthorizer for Deny {
    fn authorize(
        &self,
        _principal: &PrincipalId,
        _resource: &EnterpriseResource,
    ) -> Result<(), HostProblem> {
        Err(HostProblem::Unauthorized)
    }
}

#[test]
fn typed_topology_routes_aliases_and_fails_closed_without_mutation() {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let service = MqService::open(store.clone(), MqLimits::default()).unwrap();
    let receipt = service.install_object_catalog(catalog()).unwrap();
    assert_eq!(receipt.queues, 2);
    assert_eq!(receipt.triggers, 1);
    assert!(!receipt.replayed);
    assert!(service.install_object_catalog(catalog()).unwrap().replayed);
    assert_eq!(
        service.object_catalog().unwrap().unwrap().encode().unwrap(),
        catalog().encode().unwrap()
    );
    let no_op_uow = request(MqOperation::Rollback, "queue", 30);
    assert_eq!(
        service.execute(&invocation("empty-uow"), &no_op_uow),
        service.execute(&invocation("empty-uow"), &no_op_uow)
    );
    assert!(
        store
            .get_provider_state("mq-v1-replay", "object-effect-30")
            .unwrap()
            .is_some()
    );

    let caller = invocation("alias-run");
    let opened = service
        .execute(&caller, &request(MqOperation::Open, "alias", 1))
        .unwrap();
    let mut put = request(MqOperation::Put, "alias", 2);
    put.handle = opened.handle;
    put.message = b"payload".to_vec();
    let result = service.execute(&caller, &put).unwrap();
    assert_eq!(result.trigger_program.as_deref(), Some("PROC"));
    assert_eq!(service.queue_messages("queue").unwrap(), [b"payload"]);
    assert_eq!(service.queue_depth("alias"), Ok(1));
    assert_eq!(service.queue_depth("QUEUE"), Err(HostProblem::NotFound));

    let catalog_row = store
        .get_provider_state("mq-v1-object-catalog", "catalog")
        .unwrap()
        .unwrap();
    let topic_as_queue = service
        .execute(&caller, &request(MqOperation::Open, "TOPIC.A", 12))
        .unwrap();
    assert_eq!(
        (topic_as_queue.completion_code, topic_as_queue.reason_code),
        (2, 2085)
    );
    let replay_count = store.list_provider_state("mq-v1-replay", 20).unwrap().len();
    for unsupported in ["REMOTE.Q", "MODEL.Q"] {
        let candidate = request(MqOperation::Open, unsupported, 10 + replay_count as u64);
        assert_eq!(
            service.execute(&caller, &candidate),
            Err(HostProblem::ProviderFailure)
        );
    }
    assert_eq!(service.queue_depth("queue"), Ok(1));
    assert_eq!(
        store
            .get_provider_state("mq-v1-object-catalog", "catalog")
            .unwrap()
            .unwrap(),
        catalog_row
    );
    assert_eq!(
        store.list_provider_state("mq-v1-replay", 20).unwrap().len(),
        replay_count
    );

    drop(service);
    let denied =
        MqService::open_authorized(store.clone(), MqLimits::default(), Arc::new(Deny)).unwrap();
    let mut denied_put = request(MqOperation::PutOne, "alias", 20);
    denied_put.message = b"forbidden".to_vec();
    assert_eq!(
        denied.execute(&invocation("denied-run"), &denied_put),
        Err(HostProblem::Unauthorized)
    );
    assert_eq!(denied.queue_depth("queue"), Ok(1));
    assert_eq!(
        store.list_provider_state("mq-v1-replay", 20).unwrap().len(),
        replay_count
    );
    assert_eq!(
        denied.install(vec![MqQueueDefinition {
            name: "other".into(),
            trigger_program: None,
        }]),
        Err(HostProblem::IdempotencyConflict)
    );
}

struct SqliteFile(PathBuf);

impl SqliteFile {
    fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("mq-1502-{}-{now}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        Self(directory.join("state.sqlite"))
    }

    fn open(&self) -> Arc<dyn ProviderStateStore> {
        Arc::new(
            SqliteStateStore::open(
                &format!("sqlite://{}?mode=rwc", self.0.display()),
                64 * 1024 * 1024,
                262_144,
            )
            .unwrap(),
        )
    }
}

impl Drop for SqliteFile {
    fn drop(&mut self) {
        std::fs::remove_dir_all(self.0.parent().unwrap()).unwrap();
    }
}

#[test]
fn sqlite_process_reopen_preserves_catalog_alias_and_replay() {
    let file = SqliteFile::new();
    let caller = invocation("sqlite-object");
    let mut put = request(MqOperation::PutOne, "alias", 40);
    put.message = b"durable".to_vec();
    let expected;
    {
        let store = file.open();
        let service = MqService::open(store.clone(), MqLimits::default()).unwrap();
        service.install_object_catalog(catalog()).unwrap();
        service.inject_unknown_outcome_once();
        assert_eq!(
            service.execute(&caller, &put),
            Err(HostProblem::UnknownOutcome)
        );
        expected = service.execute(&caller, &put).unwrap();
        assert_eq!(service.queue_messages("queue").unwrap(), [b"durable"]);
        assert_eq!(
            store
                .get_provider_state("mq-v1-object-catalog", "catalog")
                .unwrap()
                .unwrap()
                .version,
            1
        );
    }
    {
        let store = file.open();
        let service = MqService::open(store.clone(), MqLimits::default()).unwrap();
        assert_eq!(
            service.object_catalog().unwrap().unwrap().encode().unwrap(),
            catalog().encode().unwrap()
        );
        assert_eq!(service.execute(&caller, &put).unwrap(), expected);
        assert_eq!(service.queue_messages("alias").unwrap(), [b"durable"]);
        let row = store
            .get_provider_state("mq-v1-object-catalog", "catalog")
            .unwrap()
            .unwrap();
        let corrupt = serde_json::json!({
            "schema_version":"mainframe-env.mq-object-row@1",
            "object_key":"catalog",
            "value":"{\"schema_version\":\"future\"}"
        });
        store
            .put_provider_state(
                ProviderStateRecord {
                    version: row.version + 1,
                    payload: serde_json::to_vec(&corrupt).unwrap(),
                    ..row
                },
                Some(1),
            )
            .unwrap();
    }
    let store = file.open();
    assert!(matches!(
        MqService::open(store.clone(), MqLimits::default()),
        Err(HostProblem::InfrastructureFailure)
    ));
    assert_eq!(
        store.list_provider_state("mq-v1-queue", 10).unwrap().len(),
        2
    );
}

#[test]
fn sqlite_reopens_and_migrates_queue_only_v1_rows() {
    let file = SqliteFile::new();
    {
        let store = file.open();
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "mq-state".into(),
                    key: "queues".into(),
                    version: 1,
                    payload: serde_json::to_vec(&serde_json::json!({
                        "schema_version":"mainframe-env.mq-row-store@1",
                        "definitions":[{"name":"Legacy.Q","trigger_program":null}],
                        "next_handle":1
                    }))
                    .unwrap(),
                },
                None,
            )
            .unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "mq-v1-queue".into(),
                    key: "Legacy.Q".into(),
                    version: 1,
                    payload: serde_json::to_vec(&serde_json::json!({
                        "schema_version":"mainframe-env.mq-object-row@1",
                        "object_key":"Legacy.Q",
                        "value":{"trigger_program":null,"messages":[]}
                    }))
                    .unwrap(),
                },
                None,
            )
            .unwrap();
    }
    {
        let store = file.open();
        let service = MqService::open(store.clone(), MqLimits::default()).unwrap();
        assert_eq!(service.queue_depth("Legacy.Q"), Ok(0));
        assert!(service.object_catalog().unwrap().is_some());
        assert!(
            service
                .install(vec![MqQueueDefinition {
                    name: "Legacy.Q".into(),
                    trigger_program: None,
                }])
                .unwrap()
                .replayed
        );
        let manifest = store
            .get_provider_state("mq-state", "queues")
            .unwrap()
            .unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(&manifest.payload).unwrap();
        assert!(manifest["definitions"].is_null());
        assert_eq!(
            store
                .get_provider_state("mq-v1-object-catalog", "catalog")
                .unwrap()
                .unwrap()
                .version,
            1
        );
    }
    let store = file.open();
    let service = MqService::open(store.clone(), MqLimits::default()).unwrap();
    assert_eq!(service.queue_depth("Legacy.Q"), Ok(0));
    assert_eq!(
        store.list_provider_state("mq-v1-queue", 2).unwrap().len(),
        1
    );
}

#[test]
fn local_queue_limit_rejects_typed_install_before_rows() {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let limits = MqLimits {
        max_queues: 1,
        ..MqLimits::default()
    };
    let service = MqService::open(store.clone(), limits).unwrap();
    assert_eq!(
        service.install_object_catalog(catalog()),
        Err(HostProblem::ResourceExhausted)
    );
    assert!(
        store
            .list_provider_state("mq-v1-queue", 10)
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .list_provider_state("mq-v1-object-catalog", 10)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn precreated_dynamic_instance_is_rejected_before_any_row_is_written() {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let service = MqService::open(store.clone(), MqLimits::default()).unwrap();
    let mut topology = catalog();
    topology
        .create_model_instance(
            &name("MODEL.Q"),
            &MqDynamicQueuePattern::new("DYNAMIC.*").unwrap(),
            MqLifecycleOwner::new("RUN.A").unwrap(),
        )
        .unwrap();
    assert_eq!(
        service.install_object_catalog(topology),
        Err(HostProblem::ProviderFailure)
    );
    assert!(
        store
            .list_provider_state("mq-v1-object-catalog", 2)
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .list_provider_state("mq-v1-queue", 2)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn concurrent_catalog_install_is_cas_fenced() {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let first = MqService::open(store.clone(), MqLimits::default()).unwrap();
    let stale = MqService::open(store.clone(), MqLimits::default()).unwrap();
    let expected = catalog();
    first.install_object_catalog(expected.clone()).unwrap();
    let alternate = MqObjectCatalog::new(
        MqQueueManagerDefinition {
            name: name("OTHER.QM"),
            default_transmission_queue: Some(name("XMIT.Q")),
        },
        expected.definitions().cloned().collect(),
        MqObjectLimits::default(),
    )
    .unwrap();
    assert_eq!(
        stale.install_object_catalog(alternate),
        Err(HostProblem::IdempotencyConflict)
    );
    let reopened = MqService::open(store, MqLimits::default()).unwrap();
    assert_eq!(
        reopened
            .object_catalog()
            .unwrap()
            .unwrap()
            .encode()
            .unwrap(),
        expected.encode().unwrap()
    );
    assert_eq!(reopened.queue_depth("queue"), Ok(0));
}
