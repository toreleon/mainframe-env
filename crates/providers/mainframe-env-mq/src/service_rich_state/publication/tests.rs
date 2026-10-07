use super::*;
use crate::{MqLocalQueueUsage, MqObjectDefinition, MqObjectName, MqQueueManagerDefinition};
use mainframe_env_execution_api::{
    ArtifactRef, AuditDecision, AuditRecord, ExecutionId, PrincipalId, RunUnitId, Selector,
};
use mainframe_env_host_api::{MqExpiry, MqGetContract, MqGetMode, MqTruncation, MqWait};
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
            "mq-rich-fence-{}-{}",
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
    r
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
fn rich_from(records: Vec<ProviderStateRecord>, fence: u64) -> RichStoredState {
    let StoredAuthority::Rich(r) = decode_records(records, 3, fence, Default::default()).unwrap()
    else {
        panic!("rich expected")
    };
    r
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
fn memory_sqlite_fence_publication_reopens_and_preserves_all_live_members_and_replay() {
    for sqlite in [false, true] {
        let mut b = Backend::new(sqlite);
        let initial = fixture(&b);
        // Seed pending puts/removed gets, decisions and a live cursor using the
        // established kernel and row authority, then capture that generation.
        let mut k = initial.delivery.clone();
        let m = k
            .get(
                &initial.catalog,
                &name("A"),
                &request(MqGetMode::Remove),
                Some(44),
            )
            .unwrap()
            .message
            .unwrap();
        k.put_one(&initial.catalog, &name("B"), m.clone(), Some(45))
            .unwrap();
        k.commit(45).unwrap();
        k.put_one(&initial.catalog, &name("B"), m.clone(), Some(46))
            .unwrap();
        k.backout(46).unwrap();
        k.get(
            &initial.catalog,
            &name("A"),
            &request(MqGetMode::BrowseFirst),
            None,
        )
        .unwrap();
        let mut expiring = m;
        expiring.descriptor.persistence = MqPersistence::NonPersistent;
        expiring.descriptor.expiry = MqExpiry::RelativeHostTicks(50);
        k.put_one(&initial.catalog, &name("A"), expiring, None)
            .unwrap();
        b.store()
            .mutate_provider_states_atomic(
                initial
                    .rows
                    .delta(&k, &initial.catalog, initial.marker.identity.clone())
                    .unwrap()
                    .into_parts()
                    .0,
            )
            .unwrap();
        let current = loaded(&b, 5);
        let p = audit_fixture(&b);
        let before = snapshot(&b, &p);
        let live = current.delivery.encode_live_checkpoint().unwrap();
        let plan = current.plan_next_fence(Default::default()).unwrap();
        assert_eq!(plan.mutations().len(), 3);
        assert_eq!(snapshot(&b, &p), before, "planning is read-only");
        assert_eq!(current.marker.identity.generation_and_fence(), (3, 5));
        let next = publish(&b, p.clone(), plan);
        assert_eq!(next.marker.identity.generation_and_fence(), (3, 6));
        assert_eq!(next.delivery.encode_live_checkpoint().unwrap(), live);
        assert_eq!(
            current.delivery.encode_live_checkpoint().unwrap(),
            live,
            "no premature adoption"
        );
        let after = records(&b);
        for old in &before.0 {
            if old.namespace.starts_with(PREFIX) && old.namespace != "mq-delivery-live-v1-meta"
                || old.namespace == REPLAY_NAMESPACE
            {
                assert!(after.contains(old), "unchanged member/replay {:?}", old);
            }
        }
        assert!(read(b.store(), 3, 5, Default::default()).is_err());
        assert_eq!(
            b.store()
                .audit_records(&p.intent.execution_id, 1, 1)
                .unwrap(),
            vec![p.audit.clone()]
        );
        assert_eq!(
            b.store().effect(&p.intent.key).unwrap(),
            Some(p.intent.clone()),
            "core completion remains coordinator-owned"
        );
        b.reopen();
        let reopened = loaded(&b, 6);
        assert_eq!(reopened.delivery.encode_live_checkpoint().unwrap(), live);
        assert_eq!(reopened.versions, next.versions);
        assert_eq!(reopened.retained_records, next.retained_records);
    }
}

#[test]
fn memory_sqlite_ordinary_and_fence_races_in_both_orders_have_exactly_one_winner() {
    for sqlite in [false, true] {
        for fence_first in [false, true] {
            let b = Backend::new(sqlite);
            let r = fixture(&b);
            let p = audit_fixture(&b);
            let ordinary = r.plan_delivery(&candidate(&r), Default::default()).unwrap();
            let fence = r.plan_next_fence(Default::default()).unwrap();
            let (winner, loser) = if fence_first {
                (fence, ordinary)
            } else {
                (ordinary, fence)
            };
            let next = publish(&b, p.clone(), winner);
            let before = snapshot(&b, &p);
            let (mutations, uncommitted) = loser.into_parts();
            let mut failed = p.clone();
            failed.mutations = mutations;
            assert_eq!(
                b.store().publish_provider_states_audited(failed),
                Err(StoreError::Conflict)
            );
            assert_eq!(snapshot(&b, &p), before);
            assert_eq!(r.marker.identity.generation_and_fence(), (3, 5));
            assert_eq!(r.delivery.depth(&name("A")), Some(2));
            let loaded = loaded(&b, if fence_first { 6 } else { 5 });
            assert_eq!(
                loaded.delivery.encode_live_checkpoint().unwrap(),
                next.delivery.encode_live_checkpoint().unwrap()
            );
            assert_ne!(
                loaded.delivery.encode_live_checkpoint().unwrap(),
                uncommitted.delivery.encode_live_checkpoint().unwrap()
            );
        }
    }
}

#[test]
fn final_marker_or_catalog_dependency_conflict_rolls_back_earlier_rows_and_audit() {
    for sqlite in [false, true] {
        for ns in [STATE_NAMESPACE, CATALOG_NAMESPACE] {
            let b = Backend::new(sqlite);
            let r = fixture(&b);
            let p = audit_fixture(&b);
            let plan = r.plan_delivery(&candidate(&r), Default::default()).unwrap();
            let key = if ns == STATE_NAMESPACE {
                STATE_KEY
            } else {
                CATALOG_KEY
            };
            let mut record = b.store().get_provider_state(ns, key).unwrap().unwrap();
            let old = record.version;
            record.version += 1;
            b.store().put_provider_state(record, Some(old)).unwrap();
            let before = snapshot(&b, &p);
            let (mutations, next) = plan.into_parts();
            let mut failed = p.clone();
            failed.mutations = mutations;
            assert_eq!(
                b.store().publish_provider_states_audited(failed),
                Err(StoreError::Conflict)
            );
            assert_eq!(snapshot(&b, &p), before);
            assert_eq!(r.delivery.depth(&name("A")), Some(2));
            assert_eq!(next.delivery.depth(&name("A")), Some(1));
        }
    }
}

#[test]
fn actual_audit_saturation_rolls_back_rows_marker_metadata_replay_and_current_authority() {
    for sqlite in [false, true] {
        let b = Backend::new(sqlite);
        let r = fixture(&b);
        let p = audit_fixture(&b);
        let next = publish(
            &b,
            p.clone(),
            r.plan_next_fence(Default::default()).unwrap(),
        );
        if sqlite {
            for i in 0..64 {
                let record = ProviderStateRecord {
                    namespace: "fence-test-quota".into(),
                    key: i.to_string(),
                    version: 1,
                    payload: vec![1],
                };
                if b.store().put_provider_state(record, None) == Err(StoreError::CapacityExceeded) {
                    break;
                }
            }
        }
        let before = snapshot(&b, &p);
        let plan = next
            .plan_delivery(&candidate(&next), Default::default())
            .unwrap();
        let (mutations, uncommitted) = plan.into_parts();
        let mut failed = p.clone();
        failed.mutations = mutations;
        assert_eq!(
            b.store().publish_provider_states_audited(failed),
            Err(StoreError::CapacityExceeded)
        );
        assert_eq!(snapshot(&b, &p), before);
        assert_eq!(next.delivery.depth(&name("A")), Some(2));
        assert_eq!(uncommitted.delivery.depth(&name("A")), Some(1));
        assert_eq!(
            loaded(&b, 6).delivery.encode_live_checkpoint().unwrap(),
            next.delivery.encode_live_checkpoint().unwrap()
        );
    }
}

#[test]
fn invalid_cached_identity_catalog_versions_and_candidates_fail_planning_without_writes() {
    let b = Backend::new(false);
    fixture(&b);
    let before = records(&b);
    for mutation in 0..4 {
        let mut r = loaded(&b, 5);
        match mutation {
            0 => r.marker.identity = r.marker.identity.next_fence().unwrap(),
            1 => r.marker.identity = DeliveryRowIdentity::new(&r.catalog, 4, 5).unwrap(),
            2 => {
                r.versions
                    .insert((STATE_NAMESPACE.into(), STATE_KEY.into()), 19);
            }
            3 => {
                r.catalog = Arc::new(
                    MqObjectCatalog::new(
                        MqQueueManagerDefinition {
                            name: name("FOREIGN.QM"),
                            default_transmission_queue: None,
                        },
                        vec![],
                        Default::default(),
                    )
                    .unwrap(),
                )
            }
            _ => unreachable!(),
        }
        assert!(r.plan_next_fence(Default::default()).is_err());
    }
    let r = loaded(&b, 5);
    let foreign = MqObjectCatalog::new(
        MqQueueManagerDefinition {
            name: name("FOREIGN.QM"),
            default_transmission_queue: None,
        },
        vec![],
        Default::default(),
    )
    .unwrap();
    let bad = MqDeliveryKernel::new(
        &foreign,
        Default::default(),
        Default::default(),
        MqPersistence::Persistent,
    )
    .unwrap();
    assert!(r.plan_delivery(&bad, Default::default()).is_err());
    assert_eq!(records(&b), before);
}

#[test]
fn exhausted_fence_and_any_dependency_version_reject_without_publication() {
    let b = Backend::new(false);
    fixture(&b);
    let original = records(&b);
    let mut exhausted = original.clone();
    for r in &mut exhausted {
        if r.namespace == STATE_NAMESPACE || r.namespace == "mq-delivery-live-v1-meta" {
            let mut v: Value = serde_json::from_slice(&r.payload).unwrap();
            if r.namespace == STATE_NAMESPACE {
                v["identity"]["fence"] = json!(i64::MAX as u64);
            } else {
                v["value"]["identity"]["fence"] = json!(i64::MAX as u64);
            }
            r.payload = serde_json::to_vec(&v).unwrap();
        }
    }
    let r = rich_from(exhausted, i64::MAX as u64);
    assert!(r.plan_next_fence(Default::default()).is_err());
    for ns in [
        STATE_NAMESPACE,
        CATALOG_NAMESPACE,
        "mq-delivery-live-v1-meta",
    ] {
        let mut exhausted = original.clone();
        exhausted
            .iter_mut()
            .find(|r| r.namespace == ns)
            .unwrap()
            .version = i64::MAX as u64;
        let r = rich_from(exhausted, 5);
        assert!(r.plan_delivery(&candidate(&r), Default::default()).is_err());
        assert!(r.plan_next_fence(Default::default()).is_err());
    }
    assert_eq!(records(&b), original);
}

#[test]
fn quota_accounting_includes_dependencies_and_never_silently_batches() {
    let b = Backend::new(false);
    let r = fixture(&b);
    let before = records(&b);
    let plan = r.plan_next_fence(Default::default()).unwrap();
    let bytes = plan
        .mutations()
        .iter()
        .map(|m| match m {
            ProviderStateMutation::Put(w) => w.record.payload.len(),
            _ => 0,
        })
        .sum::<usize>();
    let largest = plan
        .mutations()
        .iter()
        .map(|m| match m {
            ProviderStateMutation::Put(w) => w.record.payload.len(),
            _ => 0,
        })
        .max()
        .unwrap();
    for limits in [
        PublicationLimits {
            mutations: 2,
            ..Default::default()
        },
        PublicationLimits {
            mutations: usize::MAX,
            ..Default::default()
        },
        PublicationLimits {
            row_bytes: largest - 1,
            ..Default::default()
        },
        PublicationLimits {
            total_bytes: bytes - 1,
            ..Default::default()
        },
        PublicationLimits {
            total_bytes: usize::MAX,
            ..Default::default()
        },
    ] {
        assert!(r.plan_next_fence(limits).is_err());
    }
    assert!(
        r.plan_next_fence(PublicationLimits {
            mutations: 3,
            row_bytes: largest,
            total_bytes: bytes
        })
        .is_ok()
    );
    // A real bounded kernel transition can change more rows than this profile
    // admits, even though its live/checkpoint and row limits allow it.
    let mut k = r.delivery.clone();
    let m = k
        .get(
            &r.catalog,
            &name("A"),
            &request(MqGetMode::BrowseFirst),
            None,
        )
        .unwrap()
        .message
        .unwrap();
    for unit in 1..=1024 {
        k.put_one(&r.catalog, &name("B"), m.clone(), Some(unit))
            .unwrap();
    }
    assert_eq!(
        r.plan_delivery(&k, Default::default()).err(),
        Some(PublicationError::Bounds)
    );
    assert!(PublicationLimits::default().mutations < MAX_AUDITED_PROVIDER_MUTATIONS);
    assert_eq!(records(&b), before);
}

#[test]
fn stale_or_finalized_core_intent_and_composed_trailing_failure_have_no_partial_audit() {
    for sqlite in [false, true] {
        let b = Backend::new(sqlite);
        let r = fixture(&b);
        let p = audit_fixture(&b);
        let plan = r.plan_delivery(&candidate(&r), Default::default()).unwrap();
        let (mutations, _) = plan.into_parts();
        let before = snapshot(&b, &p);
        let mut failed = p.clone();
        failed.mutations = mutations.clone();
        failed.intent.request_digest[0] ^= 1;
        assert_eq!(
            b.store().publish_provider_states_audited(failed),
            Err(StoreError::Conflict)
        );
        assert_eq!(snapshot(&b, &p), before);
        let mut failed = p.clone();
        failed.mutations = mutations;
        failed
            .mutations
            .push(ProviderStateMutation::Put(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "fence-test-tail".into(),
                    key: "missing".into(),
                    version: 2,
                    payload: vec![1],
                },
                expected_version: Some(1),
            }));
        assert!(b.store().publish_provider_states_audited(failed).is_err());
        assert_eq!(snapshot(&b, &p), before);
        let mut completed = p.intent.clone();
        completed.state = EffectState::Completed;
        completed.result_digest = Some([8; 32]);
        completed.resolved_tick = Some(10);
        b.store().record_result(&p.intent.key, completed).unwrap();
        let before = snapshot(&b, &p);
        let mut failed = p.clone();
        failed.mutations = r
            .plan_next_fence(Default::default())
            .unwrap()
            .into_parts()
            .0;
        assert_eq!(
            b.store().publish_provider_states_audited(failed),
            Err(StoreError::Conflict)
        );
        assert_eq!(snapshot(&b, &p), before);
    }
}
