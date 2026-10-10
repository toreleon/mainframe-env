use super::*;
use crate::{MqLocalQueueUsage, MqObjectDefinition, MqObjectName, MqQueueManagerDefinition};
use mainframe_env_execution_api::{
    ArtifactRef, AuditDecision, AuditRecord, ExecutionId, PrincipalId, RunUnitId, Selector,
};
use mainframe_env_host_api::{MqGetContract, MqGetMode, MqTruncation, MqWait};
use mainframe_env_store::{MemoryStore, SqliteStateStore, StoreLimits};
use mainframe_env_store_api::{
    AuditedProviderPublication, EffectDigestFormat, EffectIntentMetadata, EffectRecord,
    EffectState, ExecutionRecord, ExecutionState, PlatformStore,
};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;

static NEXT_DB: AtomicU64 = AtomicU64::new(1);
struct Backend {
    store: Option<Arc<dyn PlatformStore>>,
    directory: Option<PathBuf>,
}
impl Backend {
    fn new(sqlite: bool) -> Self {
        if !sqlite {
            return Self {
                store: Some(Arc::new(MemoryStore::new(StoreLimits {
                    max_audits: 1,
                    ..Default::default()
                }))),
                directory: None,
            };
        }
        let dir = std::env::temp_dir().join(format!(
            "mq-full-upgrade-{}-{}",
            std::process::id(),
            NEXT_DB.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let store = SqliteStateStore::open(
            &format!("sqlite://{}?mode=rwc", dir.join("state.sqlite").display()),
            64 << 20,
            64,
        )
        .unwrap();
        Self {
            store: Some(Arc::new(store)),
            directory: Some(dir),
        }
    }
    fn store(&self) -> &dyn PlatformStore {
        &**self.store.as_ref().unwrap()
    }
    fn arc(&self) -> Arc<dyn ProviderStateStore> {
        self.store.as_ref().unwrap().clone()
    }
    fn reopen(&mut self) {
        if let Some(dir) = &self.directory {
            assert_eq!(Arc::strong_count(self.store.as_ref().unwrap()), 1);
            drop(self.store.take());
            self.store = Some(Arc::new(
                SqliteStateStore::open(
                    &format!("sqlite://{}?mode=rw", dir.join("state.sqlite").display()),
                    64 << 20,
                    64,
                )
                .unwrap(),
            ));
        }
    }
}
impl Drop for Backend {
    fn drop(&mut self) {
        drop(self.store.take());
        if let Some(dir) = &self.directory {
            for file in ["state.sqlite", "state.sqlite-wal", "state.sqlite-shm"] {
                let p = dir.join(file);
                if p.exists() {
                    std::fs::remove_file(p).unwrap();
                }
            }
            std::fs::remove_dir(dir).unwrap();
        }
    }
}
fn name(s: &str) -> MqObjectName {
    MqObjectName::new(s).unwrap()
}
fn object(ns: &str, key: &str, value: Value) -> ProviderStateRecord {
    ProviderStateRecord {
        namespace: ns.into(),
        key: key.into(),
        version: 1,
        payload: serde_json::to_vec(
            &json!({"schema_version":OBJECT_ROW_SCHEMA,"object_key":key,"value":value}),
        )
        .unwrap(),
    }
}
fn fixture(b: &Backend) -> RichStoredState {
    let catalog = MqObjectCatalog::new(
        MqQueueManagerDefinition {
            name: name("FENCE.QM"),
            default_transmission_queue: None,
        },
        vec![
            MqObjectDefinition::LocalQueue {
                name: name("A"),
                usage: MqLocalQueueUsage::Normal,
                trigger_process: Some(name("P")),
            },
            MqObjectDefinition::LocalQueue {
                name: name("B"),
                usage: MqLocalQueueUsage::Normal,
                trigger_process: None,
            },
            MqObjectDefinition::Process { name: name("P") },
        ],
        Default::default(),
    )
    .unwrap();
    let mut replay = object(
        REPLAY_NAMESPACE,
        "old-open",
        json!({"request_sha256":vec![7;32],"completion_code":0,"reason_code":0,"handle":19,
        "message":[0,255,81],"message_id":null,"correlation_id":null,"trigger_program":null}),
    );
    replay.payload.insert(0, b' ');
    for record in [ProviderStateRecord {namespace:STATE_NAMESPACE.into(),key:STATE_KEY.into(),version:1,
        payload:br#"{"schema_version":"mainframe-env.mq-row-store@1","definitions":null,"next_handle":20}"#.to_vec()},
        object(CATALOG_NAMESPACE,CATALOG_KEY,json!(String::from_utf8(catalog.encode().unwrap()).unwrap())),
        object(QUEUE_NAMESPACE,"A",json!({"trigger_program":"P","messages":[
            {"data":[0,255,31],"message_id":vec![1;24],"correlation_id":vec![2;24]},
            {"data":[83,69,67,79,78,68],"message_id":vec![3;24],"correlation_id":vec![4;24]}]})),
        object(QUEUE_NAMESPACE,"B",json!({"trigger_program":null,"messages":[]})),replay] {
        b.store().put_provider_state(record,None).unwrap();
    }
    let service = MqService::open(b.arc(), Default::default()).unwrap();
    let (batch, _, _, _) = service
        .plan_legacy_delivery_import(3, 5, legacy_delivery_import::LegacyImportLimits::default())
        .unwrap()
        .into_parts();
    b.store().mutate_provider_states_atomic(batch).unwrap();
    loaded(b, 5)
}
fn loaded(b: &Backend, fence: u64) -> RichStoredState {
    let StoredAuthority::Rich(r) = read(b.store(), 3, fence, Default::default()).unwrap() else {
        panic!("rich expected")
    };
    *r
}
fn request(mode: MqGetMode) -> MqGetContract {
    MqGetContract {
        selection: Default::default(),
        mode,
        wait: MqWait::NoWait,
        truncation: MqTruncation::Reject,
        buffer_capacity: 1024,
    }
}
fn candidate(r: &RichStoredState) -> MqDeliveryKernel {
    let mut k = r.delivery.clone();
    k.get(&r.catalog, &name("A"), &request(MqGetMode::Remove), None)
        .unwrap();
    k
}
fn records(b: &Backend) -> Vec<ProviderStateRecord> {
    b.store().list_provider_state_prefix("mq-", 2048).unwrap()
}
fn audit_fixture(b: &Backend) -> AuditedProviderPublication {
    let l = InvocationLimits::default();
    let e = ExecutionRecord {
        execution_id: ExecutionId::new("fence-exec", l).unwrap(),
        run_unit_id: RunUnitId::new("fence-run", l).unwrap(),
        principal: PrincipalId::new("ISSUER", l).unwrap(),
        selector: Selector::new("program:FENCE", l).unwrap(),
        artifact: ArtifactRef::new(format!("sha256:{}", "a".repeat(64)), l).unwrap(),
        state: ExecutionState::Admitted,
        attempt: 1,
        version: 1,
        owner_lease: None,
        lease_expiry_tick: None,
        terminal_tick: None,
    };
    b.store().create_execution(e.clone()).unwrap();
    b.store()
        .transition_execution(&e.execution_id, 1, ExecutionState::Queued, 6)
        .unwrap();
    b.store()
        .transition_execution(&e.execution_id, 2, ExecutionState::Running, 7)
        .unwrap();
    let request = MqRequest {
        operation: MqOperation::Get,
        queue: Some("A".into()),
        handle: None,
        options: 0,
        message: vec![],
        message_id: None,
        correlation_id: None,
        wait_ticks: 0,
        max_message_bytes: 1024,
        mutation: Some(mainframe_env_host_api::Mutation {
            sequence: 3,
            idempotency_key: IdempotencyKey::new("effect:fence:3", l).unwrap(),
            transaction: None,
        }),
    };
    let audit = AuditRecord {
        execution_id: e.execution_id.clone(),
        run_unit_id: e.run_unit_id.clone(),
        attempt: 1,
        effect_sequence: 3,
        observed_tick: 9,
        principal: e.principal,
        invocation_key: IdempotencyKey::new("invocation:fence", l).unwrap(),
        capability: CapabilityId::new("host.mq", l).unwrap(),
        resource: mainframe_env_host_api::canonical_audit_resource_digest(&HostRequest::Mq(
            request.clone(),
        )),
        decision: AuditDecision::Success,
    };
    let intent = EffectRecord {
        execution_id: e.execution_id.clone(),
        run_unit_id: e.run_unit_id,
        sequence: 3,
        key: request.mutation.as_ref().unwrap().idempotency_key.clone(),
        digest_format: EffectDigestFormat::CanonicalHostV1,
        request_digest: mainframe_env_host_api::canonical_request_digest(&HostRequest::Mq(request))
            .unwrap(),
        intent: EffectIntentMetadata {
            owner: e.execution_id,
            attempt: 1,
            capability: Some(audit.capability.clone()),
            audit_resource: Some(audit.resource),
            audit_invocation_key: Some(audit.invocation_key.clone()),
            created_tick: 8,
            recovery_after_tick: 30,
            epoch: 4,
            recovery_lease: None,
        },
        state: EffectState::Intent,
        result_digest: None,
        resolved_tick: None,
    };
    b.store().record_intent(intent.clone()).unwrap();
    AuditedProviderPublication {
        intent,
        audit,
        observed_tick: 9,
        mutations: vec![],
    }
}
fn snapshot(
    b: &Backend,
    p: &AuditedProviderPublication,
) -> (
    Vec<ProviderStateRecord>,
    Vec<AuditRecord>,
    EffectRecord,
    ExecutionRecord,
    u64,
) {
    let mut rows = records(b);
    for prefix in ["durable-", "fence-test-", "jes-worker-meta"] {
        rows.extend(b.store().list_provider_state_prefix(prefix, 2048).unwrap());
    }
    (
        rows,
        b.store()
            .audit_records(&p.intent.execution_id, 1, 1)
            .unwrap(),
        b.store().effect(&p.intent.key).unwrap().unwrap(),
        b.store()
            .get_execution(&p.intent.execution_id)
            .unwrap()
            .unwrap(),
        b.store().provider_state_retention_epoch().unwrap(),
    )
}
fn publish(
    b: &Backend,
    mut publication: AuditedProviderPublication,
    plan: RichPublicationPlan,
) -> RichStoredState {
    let (mutations, next) = plan.into_parts();
    publication.mutations = mutations;
    b.store()
        .publish_provider_states_audited(publication)
        .unwrap();
    next
}

#[test]
fn full_upgrade_memory_sqlite_cas_reopen_and_retained_corpus() {
    for sqlite in [false, true] {
        let mut b = Backend::new(sqlite);
        let r = fixture(&b);
        let p = audit_fixture(&b);
        let before = snapshot(&b, &p);
        let profile = crate::delivery::full_message::QueueProfile::Complete {
            version: 2,
            characters: mainframe_env_host_api::mq_md_value::MqMdCharacterEncoding::OwnedCp037,
        };
        let plan = r
            .plan_profile_upgrade(&BTreeMap::from([(name("B"), profile)]), Default::default())
            .unwrap();
        assert_eq!(snapshot(&b, &p), before);
        let next = publish(&b, p.clone(), plan);
        assert_eq!(r.delivery.depth(&name("A")), Some(2));
        assert_eq!(next.delivery.depth(&name("A")), Some(2));
        assert_eq!(
            next.marker, r.marker,
            "logical marker shape/identity unchanged"
        );
        for old in before.0.iter().filter(|v| v.namespace == REPLAY_NAMESPACE) {
            assert!(records(&b).contains(old));
        }
        assert_eq!(
            b.store().effect(&p.intent.key).unwrap(),
            Some(p.intent.clone())
        );
        b.reopen();
        let r = loaded(&b, 5);
        assert_eq!(
            r.delivery.encode_live_checkpoint().unwrap(),
            next.delivery.encode_live_checkpoint().unwrap()
        );
        let mut k = r.delivery.clone();
        let m = crate::delivery::full_message::tests::full(2, true, true);
        k.put_full(&r.catalog, &name("B"), m.clone(), None).unwrap();
        let plan = r
            .plan_selected_delivery(&k, Vec::new(), Default::default())
            .unwrap();
        let (batch, next) = plan.into_parts();
        b.store().mutate_provider_states_atomic(batch).unwrap();
        b.reopen();
        let r = loaded(&b, 5);
        assert_eq!(
            r.delivery.encode_live_checkpoint().unwrap(),
            next.delivery.encode_live_checkpoint().unwrap()
        );
        let mut k = r.delivery.clone();
        assert_eq!(
            k.get_full(
                &r.catalog,
                &name("B"),
                profile,
                &request(MqGetMode::Remove),
                None
            )
            .unwrap()
            .1,
            Some(m)
        );
        assert_eq!(
            k.get(&r.catalog, &name("A"), &request(MqGetMode::Remove), None)
                .unwrap()
                .message
                .unwrap()
                .body,
            vec![0, 255, 31]
        );
        // Reopen real full pending rows before the same kernel commits/backs out.
        let mut staged = r.delivery.clone();
        let exact = crate::delivery::full_message::tests::full(2, true, true);
        staged
            .get_full(
                &r.catalog,
                &name("B"),
                profile,
                &request(MqGetMode::Remove),
                Some(11),
            )
            .unwrap();
        staged
            .put_full(&r.catalog, &name("B"), exact.clone(), Some(12))
            .unwrap();
        staged
            .put_full(&r.catalog, &name("B"), exact.clone(), Some(13))
            .unwrap();
        staged
            .get_full(
                &r.catalog,
                &name("B"),
                profile,
                &request(MqGetMode::Remove),
                Some(13),
            )
            .unwrap();
        b.store()
            .mutate_provider_states_atomic(
                r.plan_selected_delivery(&staged, Vec::new(), Default::default())
                    .unwrap()
                    .into_parts()
                    .0,
            )
            .unwrap();
        b.reopen();
        let pending = loaded(&b, 5);
        assert_eq!(
            pending.delivery.unit_outcome(13),
            mainframe_env_host_api::MqDeliveryOutcome::Pending
        );
        let cold = MqDeliveryKernel::decode_stored(
            &pending.delivery.encode().unwrap(),
            &pending.catalog,
            Default::default(),
            Default::default(),
            MqPersistence::Persistent,
            true,
        )
        .unwrap();
        assert_eq!(
            cold.depth(&name("B")),
            Some(1),
            "cold retains removed persistent GET, discards staged PUT"
        );
        let mut settled = pending.delivery.clone();
        settled.commit(12).unwrap();
        settled.backout(11).unwrap();
        settled.backout(13).unwrap();
        b.store()
            .mutate_provider_states_atomic(
                pending
                    .plan_selected_delivery(&settled, Vec::new(), Default::default())
                    .unwrap()
                    .into_parts()
                    .0,
            )
            .unwrap();
        b.reopen();
        let settled = loaded(&b, 5);
        assert_eq!(settled.delivery.depth(&name("B")), Some(2));
        assert_eq!(
            b.store()
                .audit_records(&p.intent.execution_id, 1, 1)
                .unwrap(),
            vec![p.audit]
        );
    }
}
#[test]
fn full_upgrade_stale_writer_both_orderings_and_last_dependency_rollback() {
    for sqlite in [false, true] {
        for upgrade_first in [false, true] {
            let b = Backend::new(sqlite);
            let r = fixture(&b);
            let p = audit_fixture(&b);
            let upgrade = r
                .plan_profile_upgrade(&BTreeMap::new(), Default::default())
                .unwrap();
            let ordinary = r
                .plan_selected_delivery(&candidate(&r), Vec::new(), Default::default())
                .unwrap();
            let (winner, loser) = if upgrade_first {
                (upgrade, ordinary)
            } else {
                (ordinary, upgrade)
            };
            let next = publish(&b, p.clone(), winner);
            let before = snapshot(&b, &p);
            let (batch, uncommitted) = loser.into_parts();
            let mut fail = p.clone();
            fail.mutations = batch;
            assert_eq!(
                b.store().publish_provider_states_audited(fail),
                Err(StoreError::Conflict)
            );
            assert_eq!(snapshot(&b, &p), before);
            assert_eq!(
                loaded(&b, 5).delivery.encode_live_checkpoint().unwrap(),
                next.delivery.encode_live_checkpoint().unwrap()
            );
            assert_ne!(
                next.delivery.encode_live_checkpoint().unwrap(),
                uncommitted.delivery.encode_live_checkpoint().unwrap()
            );
        }
    }
    for sqlite in [false, true] {
        for ns in [STATE_NAMESPACE, CATALOG_NAMESPACE] {
            let b = Backend::new(sqlite);
            let r = fixture(&b);
            let p = audit_fixture(&b);
            let plan = r
                .plan_profile_upgrade(&BTreeMap::new(), Default::default())
                .unwrap();
            let key = if ns == STATE_NAMESPACE {
                STATE_KEY
            } else {
                CATALOG_KEY
            };
            let mut record = b.store().get_provider_state(ns, key).unwrap().unwrap();
            let v = record.version;
            record.version += 1;
            b.store().put_provider_state(record, Some(v)).unwrap();
            let before = snapshot(&b, &p);
            let mut fail = p.clone();
            fail.mutations = plan.into_parts().0;
            assert_eq!(
                b.store().publish_provider_states_audited(fail),
                Err(StoreError::Conflict)
            );
            assert_eq!(snapshot(&b, &p), before);
        }
    }
}
#[test]
fn full_upgrade_quotas_and_composed_late_row_failure_are_read_only() {
    for sqlite in [false, true] {
        let b = Backend::new(sqlite);
        let r = fixture(&b);
        let p = audit_fixture(&b);
        let before = snapshot(&b, &p);
        let profile = crate::delivery::full_message::QueueProfile::Complete {
            version: 1,
            characters: mainframe_env_host_api::mq_md_value::MqMdCharacterEncoding::AsciiCompatible,
        };
        assert!(
            r.plan_profile_upgrade(&BTreeMap::from([(name("A"), profile)]), Default::default())
                .is_err()
        );
        for limits in [
            PublicationLimits {
                mutations: 1,
                ..Default::default()
            },
            PublicationLimits {
                total_bytes: 1,
                ..Default::default()
            },
            PublicationLimits {
                row_bytes: 1,
                ..Default::default()
            },
        ] {
            assert!(r.plan_profile_upgrade(&BTreeMap::new(), limits).is_err());
        }
        assert_eq!(snapshot(&b, &p), before);
        let plan = r
            .plan_profile_upgrade(&BTreeMap::new(), Default::default())
            .unwrap();
        let mut fail = p.clone();
        fail.mutations = plan.into_parts().0;
        fail.mutations
            .push(ProviderStateMutation::Put(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "fence-test-late".into(),
                    key: "absent".into(),
                    version: 2,
                    payload: vec![1],
                },
                expected_version: Some(1),
            }));
        assert_eq!(
            b.store().publish_provider_states_audited(fail),
            Err(StoreError::Conflict)
        );
        assert_eq!(snapshot(&b, &p), before);
    }
}

#[test]
fn full_upgrade_selected_control_and_empty_live_owner_refused_without_retirement() {
    for sqlite in [false, true] {
        for empty_unit in [false, true] {
            let b = Backend::new(sqlite);
            fixture(&b);
            b.store().put_provider_state(object("mq-selected-v1-control","state",json!({"schema_version":"mainframe-env.mq-selected-control@1","generation":3,"fence":5,"registry_epoch":1,"next_unit":if empty_unit {2}else{1}})),None).unwrap();
            if empty_unit {
                b.store().put_provider_state(object("mq-selected-v1-uow-owner","1",json!({"schema_version":"mainframe-env.mq-selected-uow-owner@1","coordinator":"queue-manager-local","unit":1,"connection_key":"conn:original","execution":"original-exec","run":"original-run","principal":"ISSUER","generation":3,"fence":5,"registry_epoch":1,"state":"pending","queues":[]})),None).unwrap();
            }
            let r = loaded(&b, 5);
            let before = records(&b);
            assert!(
                r.plan_profile_upgrade(&BTreeMap::new(), Default::default())
                    .is_err()
            );
            assert_eq!(records(&b), before);
        }
    }
}

#[test]
fn full_upgrade_actual_audit_or_physical_quota_failure_never_adopts_candidate() {
    for sqlite in [false, true] {
        let b = Backend::new(sqlite);
        let r = fixture(&b);
        let p = audit_fixture(&b);
        let r = publish(
            &b,
            p.clone(),
            r.plan_selected_delivery(&r.delivery, Vec::new(), Default::default())
                .unwrap(),
        );
        if sqlite {
            for i in 0..64 {
                if b.store().put_provider_state(
                    ProviderStateRecord {
                        namespace: "fence-test-quota".into(),
                        key: i.to_string(),
                        version: 1,
                        payload: vec![1],
                    },
                    None,
                ) == Err(StoreError::CapacityExceeded)
                {
                    break;
                }
            }
        }
        let before = snapshot(&b, &p);
        let plan = r
            .plan_profile_upgrade(&BTreeMap::new(), Default::default())
            .unwrap();
        let mut failed = p.clone();
        failed.mutations = plan.into_parts().0;
        assert_eq!(
            b.store().publish_provider_states_audited(failed),
            Err(StoreError::CapacityExceeded)
        );
        assert_eq!(snapshot(&b, &p), before);
        assert_eq!(
            loaded(&b, 5).delivery.encode_live_checkpoint().unwrap(),
            r.delivery.encode_live_checkpoint().unwrap()
        );
    }
}
