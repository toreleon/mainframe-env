use super::*;
use mainframe_env_host_api::{
    ImsRecoveryCall, ImsRecoveryRequest, ImsRecoveryResult, ImsRestartSelection,
    canonical_request_digest,
};
use mainframe_env_store_api::{
    EffectDigestFormat, EffectIntentMetadata, EffectRecord, EffectState, IdempotencyStore,
};

trait RecoveryStore: ProviderStateStore + IdempotencyStore {}
impl<T: ProviderStateStore + IdempotencyStore> RecoveryStore for T {}
const PACKAGE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn recovery_call(
    service: &Arc<ImsService>,
    store: Arc<dyn RecoveryStore>,
    inv: &Invocation,
    sequence: u64,
    call: ImsRecoveryCall,
) -> ImsRecoveryResult {
    let req = ImsRecoveryRequest {
        application: "LASTAPP".into(),
        package_identity: PACKAGE.into(),
        psb: "GENPSB".into(),
        database: "GENDB".into(),
        context: ImsExecutionContext::DbBatch,
        syntax: ImsCallSyntax::Call,
        call,
        mutation: Mutation {
            sequence,
            idempotency_key: IdempotencyKey::new(
                format!("last-recovery-{sequence}"),
                InvocationLimits::default(),
            )
            .unwrap(),
            transaction: None,
        },
    };
    let host = HostRequest::ImsRecovery(req.clone());
    store
        .record_intent(EffectRecord {
            execution_id: inv.execution_id.clone(),
            run_unit_id: inv.run_unit_id.clone(),
            sequence,
            key: req.mutation.idempotency_key.clone(),
            digest_format: EffectDigestFormat::CanonicalHostV1,
            request_digest: canonical_request_digest(&host).unwrap(),
            intent: EffectIntentMetadata {
                owner: inv.execution_id.clone(),
                attempt: inv.attempt,
                capability: Some(
                    CapabilityId::new("host.ims.write", InvocationLimits::default()).unwrap(),
                ),
                audit_resource: None,
                audit_invocation_key: None,
                created_tick: 1,
                recovery_after_tick: 100,
                epoch: sequence,
                recovery_lease: None,
            },
            state: EffectState::Intent,
            result_digest: None,
            resolved_tick: None,
        })
        .unwrap();
    let result =
        crate::ims_providers_with_recovery(service.clone(), store, InvocationLimits::default())[1]
            .invoke(
                inv,
                EffectRequest {
                    run_unit: inv.run_unit_id.clone(),
                    sequence,
                    idempotency_key: Some(req.mutation.idempotency_key),
                    deadline_tick: 100,
                    request: host,
                },
            )
            .outcome
            .unwrap();
    match result {
        HostResult::ImsRecovery(r) => r,
        other => panic!("{other:?}"),
    }
}

fn checkpoint_case(store: Arc<dyn RecoveryStore>, url: Option<&str>) {
    let service = ImsService::open_authorized(
        store.clone(),
        Default::default(),
        Arc::new(Policy::default()),
    )
    .unwrap();
    let metadata = last_catalog();
    service.install_metadata(metadata.clone()).unwrap();
    service
        .publish_metadata_generation("LASTAPP", 1, PACKAGE, Some(&metadata))
        .unwrap();
    load(&service, RUN);
    db(
        service.clone(),
        RUN,
        request(RUN, ImsOperation::Commit, 903, &[], b""),
    );
    let mut inv = invocation(RUN);
    inv.service_class = ServiceClass::Batch;
    assert!(matches!(
        recovery_call(
            &service,
            store.clone(),
            &inv,
            100,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Normal,
                area_lengths: vec![3]
            }
        ),
        ImsRecoveryResult::Restarted { .. }
    ));
    nav(
        &service,
        3,
        1,
        ImsOperation::GetHoldUnique,
        &[b"ROOT    (ROOTKEY EQA1)"],
    )
    .unwrap();
    assert_eq!(
        nav(&service, 4, 1, ImsOperation::GetHoldNextParent, &[LAST])
            .unwrap()
            .segments[0]
            .data,
        b"C3S"
    );
    db(
        service.clone(),
        RUN,
        request(RUN, ImsOperation::Replace, 5, &["CHILD"], b"C3Z"),
    );
    // REPL clears parentage by the existing owner; actual GU restores the child for CHKP.
    nav(
        &service,
        6,
        1,
        ImsOperation::GetHoldUnique,
        &[b"ROOT    (ROOTKEY EQA1)", b"CHILD   (CHILDKEYEQC3)"],
    )
    .unwrap();
    assert_eq!(
        cursor(&service, 1),
        serde_json::json!({"current":5,"parentage":5,"held":{"id":5,"version":2},"after_end":false})
    );
    assert_eq!(
        recovery_call(
            &service,
            store.clone(),
            &inv,
            101,
            ImsRecoveryCall::SymbolicCheckpoint {
                id: "LASTC3".into(),
                user_areas: vec![b"XYZ".to_vec()]
            }
        ),
        ImsRecoveryResult::Checkpointed {
            status: "  ".into(),
            id: "LASTC3".into(),
            sequence: 2
        }
    );
    assert_eq!(position(&service, 1), PcbPosition::default());
    assert!(
        store
            .list_provider_state(GENERIC_PENDING_NAMESPACE, 64)
            .unwrap()
            .is_empty()
    );
    let before = rows(&*store);
    assert_eq!(
        nav(&service, 7, 1, ImsOperation::GetNextParent, &[LAST]),
        Err(HostProblem::Unsupported)
    );
    assert_eq!(rows(&*store), before);
    drop(service);
    let reopened: Arc<dyn RecoveryStore> = url.map_or_else(
        || store.clone(),
        |url| Arc::new(SqliteStateStore::open(url, 64 * 1024 * 1024, 262_144).unwrap()),
    );
    let service = ImsService::open_authorized(
        reopened.clone(),
        Default::default(),
        Arc::new(Policy::default()),
    )
    .unwrap();
    inv.execution_id = ExecutionId::new("last-restart", InvocationLimits::default()).unwrap();
    assert_eq!(
        recovery_call(
            &service,
            reopened.clone(),
            &inv,
            102,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Checkpoint("LASTC3".into()),
                area_lengths: vec![3]
            }
        ),
        ImsRecoveryResult::Restarted {
            status: "  ".into(),
            checkpoint_id: Some("LASTC3".into()),
            user_areas: vec![b"XYZ".to_vec()],
            pcb_statuses: vec![(1, "  ".into())]
        }
    );
    assert_eq!(
        cursor(&service, 1),
        serde_json::json!({"current":5,"parentage":5,"held":null,"after_end":false})
    );
    let before = rows(&*reopened);
    assert_eq!(
        nav(&service, 8, 1, ImsOperation::GetHoldNextParent, &[LAST]),
        Err(HostProblem::Unsupported)
    );
    assert_eq!(rows(&*reopened), before);
    assert_eq!(
        nav(&service, 4, 1, ImsOperation::GetHoldNextParent, &[LAST])
            .unwrap()
            .segments[0]
            .data,
        b"C3S"
    );
    assert_eq!(rows(&*reopened), before);
    nav(
        &service,
        9,
        1,
        ImsOperation::GetUnique,
        &[b"ROOT    (ROOTKEY EQA1)"],
    )
    .unwrap();
    assert_eq!(
        nav(&service, 10, 1, ImsOperation::GetHoldNextParent, &[LAST])
            .unwrap()
            .segments[0]
            .data,
        b"C3Z"
    );
    assert_eq!(
        cursor(&service, 1),
        serde_json::json!({"current":5,"parentage":2,"held":{"id":5,"version":2},"after_end":false})
    );
    assert_eq!(
        nav(&service, 11, 1, ImsOperation::GetNext, &[])
            .unwrap()
            .segments[0]
            .data,
        b"B2Y"
    );
}

#[test]
fn ssa_last_direct_child_real_chkp_xrst_child_parentage_and_committed_replace() {
    eprintln!("scenario checkpoint/memory");
    checkpoint_case(Arc::new(MemoryStore::new(Default::default())), None);
    let (file, url, store) = sqlite();
    drop(store);
    eprintln!("scenario checkpoint/sqlite-reopen");
    checkpoint_case(
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap()),
        Some(&url),
    );
    std::fs::remove_file(file).unwrap();
}
