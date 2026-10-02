use super::*;
use crate::{
    IMS_METADATA_SCHEMA_V1, ImsDatabaseOrganization, ImsDbLevel, ImsFieldMetadata,
    ImsLogicalRelationshipMetadata, ImsPsbMetadata, ImsSecondaryIndexMetadata, ImsSegmentMetadata,
    ImsSensitiveSegmentMetadata,
};
use mainframe_env_execution_api::{
    ArtifactRef, ExecutionId, Principal, PrincipalId, RequestId, ResourceLimits, RunUnitId,
    Selector, TraceId,
};
use mainframe_env_host_api::{
    ImsAcceptRow, ImsBufferPoolDefinition, ImsBufferPoolKind, ImsBufferStatistics, ImsCallSyntax,
    ImsDedbAreaDefinition, ImsExecutionContext, ImsPositionArea, ImsPositionKeyword,
    ImsPositionSsa, ImsQClass, ImsQualifier, ImsStatisticsFamily, ImsStatisticsFormat,
    ImsStatisticsFunction, ImsSystemCall, ImsSystemRequest, ImsSystemResult,
    ImsSystemRuntimeDefinition, Mutation,
};
use mainframe_env_store::{MemoryStore, SqliteStateStore};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FILE: AtomicU64 = AtomicU64::new(1);

mod basic_checkpoint_tests;
mod closure_tests;
mod isolation_tests;
mod pcb_tests;
mod ssa_tests;

pub(crate) fn catalog() -> ImsMetadataCatalog {
    fn field(name: &str, offset: usize, sequence: bool) -> ImsFieldMetadata {
        ImsFieldMetadata {
            name: Some(name.into()),
            offset,
            length: if sequence { 2 } else { 1 },
            sequence,
            unique: sequence,
        }
    }
    fn segment(name: &str, parent: Option<&str>, key: &str) -> ImsSegmentMetadata {
        ImsSegmentMetadata {
            name: name.into(),
            parent: parent.map(str::to_owned),
            min_length: 3,
            max_length: 3,
            fields: vec![field(key, 0, true), field("KIND", 2, false)],
        }
    }
    ImsMetadataCatalog {
        schema_version: IMS_METADATA_SCHEMA_V1.into(),
        databases: vec![ImsDatabaseMetadata {
            name: "GENDB".into(),
            version: 1,
            organization: ImsDatabaseOrganization::Hidam,
            segments: vec![
                segment("ROOT", None, "ROOTKEY"),
                segment("CHILD", Some("ROOT"), "CHILDKEY"),
            ],
            secondary_indexes: vec![],
            logical_relationships: vec![],
        }],
        psbs: vec![ImsPsbMetadata {
            name: "GENPSB".into(),
            database_level: ImsDbLevel::Current,
            pcbs: vec![ImsPcbMetadata::Database(ImsDatabasePcbMetadata {
                name: "GENPCB".into(),
                database: "GENDB".into(),
                database_version: Some(1),
                secondary_index: None,
                processing_options: "AP".into(),
                sensitive_segments: vec![
                    ImsSensitiveSegmentMetadata {
                        name: "ROOT".into(),
                        parent: None,
                        processing_options: None,
                    },
                    ImsSensitiveSegmentMetadata {
                        name: "CHILD".into(),
                        parent: Some("ROOT".into()),
                        processing_options: None,
                    },
                ],
            })],
        }],
    }
}

pub(crate) fn invocation(run: &str) -> Invocation {
    invocation_class(run, ServiceClass::Interactive)
}

fn invocation_class(run: &str, class: ServiceClass) -> Invocation {
    let limits = InvocationLimits::default();
    let grants = ["host.ims.read", "host.ims.write"]
        .into_iter()
        .map(|name| CapabilityId::new(name, limits).unwrap())
        .collect();
    Invocation::new(
        RequestId::new(format!("request-{run}"), limits).unwrap(),
        ExecutionId::new(format!("execution-{run}"), limits).unwrap(),
        RunUnitId::new(run, limits).unwrap(),
        None,
        Selector::new("ims:generic", limits).unwrap(),
        ArtifactRef::new("ims:generic", limits).unwrap(),
        Principal::new(PrincipalId::new("IBMUSER", limits).unwrap(), grants, limits).unwrap(),
        class,
        0,
        100,
        TraceId::new(format!("trace-{run}"), limits).unwrap(),
        IdempotencyKey::new(format!("invocation-{run}"), limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .unwrap()
}

pub(crate) fn request(
    run: &str,
    op: ImsOperation,
    sequence: u64,
    segments: &[&str],
    data: &[u8],
) -> ImsRequest {
    let limits = InvocationLimits::default();
    ImsRequest {
        operation: op,
        psb: (op == ImsOperation::Schedule).then(|| "GENPSB".into()),
        pcb: 1,
        segments: segments.iter().map(|s| (*s).into()).collect(),
        data: data.to_vec(),
        qualifiers: vec![],
        checkpoint_id: (op == ImsOperation::Checkpoint).then(|| format!("CHK-{run}")),
        max_segments: 64,
        mutation: op.is_mutating().then(|| Mutation {
            sequence,
            idempotency_key: IdempotencyKey::new(format!("{run}-{sequence}"), limits).unwrap(),
            transaction: Some("IMS-GENERIC".into()),
        }),
        system: None,
        q_class: None,
    }
}

fn execute(service: &ImsService, run: &str, req: &ImsRequest) -> ImsResult {
    service.execute(&invocation(run), req).unwrap()
}

fn qualifier(key: &[u8]) -> ImsQualifier {
    ImsQualifier {
        segment: "ROOT".into(),
        field: "ROOTKEY".into(),
        value: key.into(),
    }
}

fn exercise(store: Arc<dyn ProviderStateStore>) {
    let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    service.install_metadata(catalog()).unwrap();
    assert_eq!(service.install_metadata(catalog()).unwrap().len(), 71);
    let run = "generic-run";
    assert_eq!(
        execute(
            &service,
            run,
            &request(run, ImsOperation::Schedule, 1, &[], b"")
        )
        .status,
        "  "
    );
    assert_eq!(
        execute(
            &service,
            run,
            &request(run, ImsOperation::Insert, 2, &["ROOT"], b"A1X")
        )
        .affected_segments,
        1
    );
    assert_eq!(
        execute(
            &service,
            run,
            &request(run, ImsOperation::Insert, 3, &["ROOT"], b"B2Y")
        )
        .affected_segments,
        1
    );
    let mut hold = request(run, ImsOperation::GetHoldUnique, 4, &["ROOT"], b"");
    hold.qualifiers.push(qualifier(b"A1"));
    assert_eq!(execute(&service, run, &hold).segments[0].data, b"A1X");
    let held = service.lock().unwrap().state.sessions[run].position.clone();
    assert!(held.is_held());
    assert_eq!(
        execute(
            &service,
            run,
            &request(run, ImsOperation::Replace, 5, &[], b"A1Z")
        )
        .status,
        "  "
    );
    assert!(
        service.lock().unwrap().state.sessions[run]
            .position
            .is_held()
    );
    assert_eq!(
        execute(
            &service,
            run,
            &request(run, ImsOperation::Replace, 6, &[], b"A1Q")
        )
        .status,
        "  "
    );
    let mut failed = request(run, ImsOperation::GetUnique, 7, &["ROOT"], b"");
    failed.qualifiers.push(qualifier(b"ZZ"));
    assert_eq!(execute(&service, run, &failed).status, "GE");
    let failed_position = service.lock().unwrap().state.sessions[run].position.clone();
    assert_eq!(failed_position.current(), held.current());
    assert_eq!(failed_position.parentage(), None);
    assert!(!failed_position.is_held());
    assert_eq!(
        execute(
            &service,
            run,
            &request(run, ImsOperation::Replace, 8, &[], b"A1R")
        )
        .status,
        "DJ"
    );
    assert_eq!(
        service.lock().unwrap().state.sessions[run].position,
        failed_position
    );
    assert_eq!(
        execute(
            &service,
            run,
            &request(run, ImsOperation::GetNext, 9, &[], b"")
        )
        .segments[0]
            .data,
        b"B2Y"
    );
    assert_eq!(
        execute(
            &service,
            run,
            &request(run, ImsOperation::GetNext, 10, &[], b"")
        )
        .status,
        "GB"
    );
    assert_eq!(
        service.lock().unwrap().state.sessions[run]
            .position
            .current(),
        None
    );
    let first_insert = request(run, ImsOperation::Insert, 2, &["ROOT"], b"A1X");
    assert_eq!(execute(&service, run, &first_insert).status, "  ");
    assert_eq!(
        execute(
            &service,
            run,
            &request(run, ImsOperation::Rollback, 11, &[], b"")
        )
        .status,
        "  "
    );
    assert_eq!(
        service.lock().unwrap().state.generic_databases["GENDB"].clone(),
        Arc::new(
            DatabaseEngine::new(
                definition(&catalog().databases[0]).unwrap(),
                engine_limits(ImsLimits::default())
            )
            .unwrap()
            .image()
        )
    );
    drop(service);
    let reopened = ImsService::open(store, ImsLimits::default()).unwrap();
    assert_eq!(
        execute(
            &reopened,
            run,
            &request(run, ImsOperation::GetNext, 12, &[], b"")
        )
        .status,
        "GB"
    );
    assert_eq!(execute(&reopened, run, &first_insert).status, "  ");
    assert_eq!(
        reopened.lock().unwrap().state.generic_databases["GENDB"].clone(),
        Arc::new(
            DatabaseEngine::new(
                definition(&catalog().databases[0]).unwrap(),
                engine_limits(ImsLimits::default())
            )
            .unwrap()
            .image()
        )
    );
}

#[test]
fn public_generic_status_position_replay_rollback_and_memory_reopen() {
    exercise(Arc::new(MemoryStore::new(Default::default())));
}

#[test]
fn public_generic_status_position_replay_rollback_and_sqlite_reopen() {
    let file = std::env::temp_dir().join(format!(
        "ims-generic-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    let store: Arc<dyn ProviderStateStore> =
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    exercise(store);
    std::fs::remove_file(file).unwrap();
}

#[derive(Default)]
struct Policy {
    deny_update: Mutex<bool>,
    deny_read: Mutex<bool>,
    seen: Mutex<Vec<EnterpriseResource>>,
}

impl EnterpriseAuthorizer for Policy {
    fn authorize(&self, _: &PrincipalId, resource: &EnterpriseResource) -> Result<(), HostProblem> {
        self.seen.lock().unwrap().push(resource.clone());
        if resource.class == EnterpriseResourceClass::ImsDatabase
            && ((*self.deny_update.lock().unwrap() && resource.intent == AccessIntent::Update)
                || (*self.deny_read.lock().unwrap() && resource.intent == AccessIntent::Read))
        {
            Err(HostProblem::Unauthorized)
        } else {
            Ok(())
        }
    }
}

#[test]
fn authorization_precedes_generic_mutation_and_replay() {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let policy = Arc::new(Policy::default());
    let service = ImsService::open_authorized(store, ImsLimits::default(), policy.clone()).unwrap();
    service.install_metadata(catalog()).unwrap();
    let run = "policy-run";
    execute(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    *policy.deny_update.lock().unwrap() = true;
    let insert = request(run, ImsOperation::Insert, 2, &["ROOT"], b"A1X");
    assert_eq!(
        service.execute(&invocation(run), &insert),
        Err(HostProblem::Unauthorized)
    );
    let engine = restored(
        &service.lock().unwrap().state,
        "GENDB",
        ImsLimits::default(),
    )
    .unwrap();
    assert_eq!(engine.record_count(), 0);
    assert!(
        !service
            .lock()
            .unwrap()
            .state
            .replay
            .contains_key("policy-run-2")
    );
    assert!(
        policy
            .seen
            .lock()
            .unwrap()
            .iter()
            .any(|resource| resource.class == EnterpriseResourceClass::ImsDatabase)
    );
}

fn legacy_catalog() -> ImsApplicationDefinition {
    ImsApplicationDefinition {
        databases: vec![ImsDatabaseDefinition {
            name: "OLDDB".into(),
            access: "HIDAM".into(),
            secondary_index: None,
            segments: vec![ImsSegmentDefinition {
                name: "ROOT".into(),
                parent: None,
                length: 3,
                key_field: "ROOTKEY".into(),
                key_offset: 0,
                key_length: 2,
            }],
        }],
        psbs: vec![ImsPsbDefinition {
            name: "OLDPSB".into(),
            pcbs: vec![ImsPcbDefinition {
                name: "OLDPCB".into(),
                database: "OLDDB".into(),
                processing_options: "AP".into(),
                segments: vec!["ROOT".into()],
            }],
        }],
    }
}

fn corrupt_row(
    store: &dyn ProviderStateStore,
    namespace: &str,
    key: &str,
    field: &str,
    replacement: serde_json::Value,
) {
    let mut row = store.get_provider_state(namespace, key).unwrap().unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
    value["value"][field] = replacement;
    row.payload = serde_json::to_vec(&value).unwrap();
    let prior = row.version;
    row.version += 1;
    store.put_provider_state(row, Some(prior)).unwrap();
}

#[test]
fn coexisting_catalogs_execute_and_legacy_corruption_is_checked() {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    service.install(legacy_catalog()).unwrap();
    service.install_metadata(catalog()).unwrap();
    let run = "mixed-generic";
    execute(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    execute(
        &service,
        run,
        &request(run, ImsOperation::Insert, 2, &["ROOT"], b"A1X"),
    );
    let run = "mixed-legacy";
    let mut schedule = request(run, ImsOperation::Schedule, 1, &[], b"");
    schedule.psb = Some("OLDPSB".into());
    execute(&service, run, &schedule);
    execute(
        &service,
        run,
        &request(run, ImsOperation::Insert, 2, &["ROOT"], b"B2Y"),
    );
    assert_eq!(service.hierarchy("OLDDB").unwrap().len(), 1);
    assert_eq!(
        restored(
            &service.lock().unwrap().state,
            "GENDB",
            ImsLimits::default()
        )
        .unwrap()
        .record_count(),
        1
    );
    drop(service);
    assert!(ImsService::open(store.clone(), ImsLimits::default()).is_ok());
    corrupt_row(
        &*store,
        DATABASE_NAMESPACE,
        "OLDDB",
        "secondary_index",
        serde_json::json!({"bad":"missing"}),
    );
    assert!(matches!(
        ImsService::open(store, ImsLimits::default()),
        Err(HostProblem::InfrastructureFailure)
    ));
}

#[test]
fn corrupt_generic_image_is_rejected_on_reopen() {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    ImsService::open(store.clone(), ImsLimits::default())
        .unwrap()
        .install_metadata(catalog())
        .unwrap();
    corrupt_row(
        &*store,
        GENERIC_DATABASE_NAMESPACE,
        "GENDB",
        "next_id",
        serde_json::json!(0),
    );
    assert!(matches!(
        ImsService::open(store, ImsLimits::default()),
        Err(HostProblem::InfrastructureFailure)
    ));
}

#[test]
fn committed_generic_rows_survive_fresh_sqlite_connection() {
    let file = std::env::temp_dir().join(format!(
        "ims-generic-commit-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    {
        let store: Arc<dyn ProviderStateStore> =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let service = ImsService::open(store, ImsLimits::default()).unwrap();
        service.install_metadata(catalog()).unwrap();
        let run = "commit-run";
        execute(
            &service,
            run,
            &request(run, ImsOperation::Schedule, 1, &[], b""),
        );
        execute(
            &service,
            run,
            &request(run, ImsOperation::Insert, 2, &["ROOT"], b"A1X"),
        );
        execute(
            &service,
            run,
            &request(run, ImsOperation::Commit, 3, &[], b""),
        );
    }
    {
        let store: Arc<dyn ProviderStateStore> =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let service = ImsService::open(store, ImsLimits::default()).unwrap();
        let mut get = request("commit-run", ImsOperation::GetUnique, 4, &["ROOT"], b"");
        get.qualifiers.push(qualifier(b"A1"));
        assert_eq!(
            execute(&service, "commit-run", &get).segments[0].data,
            b"A1X"
        );
        assert!(
            service
                .lock()
                .unwrap()
                .state
                .generic_pending_undo
                .is_empty()
        );
    }
    std::fs::remove_file(file).unwrap();
}

#[test]
fn generic_child_hold_delete_index_and_bulk_image_use_typed_metadata() {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let service = ImsService::open(store, ImsLimits::default()).unwrap();
    let mut metadata = catalog();
    metadata.databases[0]
        .secondary_indexes
        .push(ImsSecondaryIndexMetadata {
            name: "ROOTKIND".into(),
            target_segment: "ROOT".into(),
            source_segment: "ROOT".into(),
            source_fields: vec!["KIND".into()],
        });
    service.install_metadata(metadata).unwrap();
    let run = "child-run";
    execute(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    execute(
        &service,
        run,
        &request(run, ImsOperation::Insert, 2, &["ROOT"], b"A1X"),
    );
    let mut root = request(run, ImsOperation::GetUnique, 3, &["ROOT"], b"");
    root.qualifiers.push(qualifier(b"A1"));
    execute(&service, run, &root);
    execute(
        &service,
        run,
        &request(run, ImsOperation::Insert, 4, &["CHILD"], b"C1Q"),
    );
    execute(
        &service,
        run,
        &request(run, ImsOperation::GetUnique, 5, &["ROOT"], b""),
    );
    let held = execute(
        &service,
        run,
        &request(run, ImsOperation::GetHoldNextParent, 6, &["CHILD"], b""),
    );
    assert_eq!(held.segments[0].data, b"C1Q");
    assert_eq!(held.segments[0].parent_key, Some(b"A1".to_vec()));
    assert_eq!(
        execute(
            &service,
            run,
            &request(run, ImsOperation::Replace, 7, &[], b"C1Z")
        )
        .status,
        "  "
    );
    assert_eq!(
        execute(
            &service,
            run,
            &request(run, ImsOperation::Delete, 8, &[], b"")
        )
        .affected_segments,
        1
    );
    assert_eq!(
        execute(
            &service,
            run,
            &request(run, ImsOperation::GetNextParent, 9, &["CHILD"], b"")
        )
        .status,
        "GE"
    );
    let engine = restored(
        &service.lock().unwrap().state,
        "GENDB",
        ImsLimits::default(),
    )
    .unwrap();
    assert_eq!(engine.lookup_index("ROOTKIND", b"X").unwrap().len(), 1);
    let image = ImsGenericLoadImage {
        database: "GENDB".into(),
        records: vec![
            ImsGenericLoadRecord {
                segment: "ROOT".into(),
                parent: None,
                data: b"D1W".to_vec(),
            },
            ImsGenericLoadRecord {
                segment: "CHILD".into(),
                parent: Some(0),
                data: b"E1V".to_vec(),
            },
        ],
    };
    let load = request(
        run,
        ImsOperation::Load,
        10,
        &[],
        &serde_json::to_vec(&image).unwrap(),
    );
    assert_eq!(execute(&service, run, &load).affected_segments, 2);
    let mut unload = request(run, ImsOperation::Unload, 11, &[], b"");
    unload.psb = Some("GENDB".into());
    assert_eq!(
        execute(&service, run, &unload)
            .segments
            .iter()
            .map(|segment| segment.data.clone())
            .collect::<Vec<_>>(),
        vec![b"D1W".to_vec(), b"E1V".to_vec()]
    );
}

fn system_request(
    run: &str,
    sequence: u64,
    pcb: u16,
    context: ImsExecutionContext,
    syntax: ImsCallSyntax,
    call: ImsSystemCall,
) -> ImsRequest {
    let mut request = request(run, ImsOperation::System, sequence, &[], b"");
    request.pcb = pcb;
    request.system = Some(ImsSystemRequest {
        context,
        syntax,
        call,
    });
    request
}

fn system_catalog() -> ImsMetadataCatalog {
    let mut result = catalog();
    let mut fast = result.databases[0].clone();
    fast.name = "FASTDB".into();
    fast.organization = ImsDatabaseOrganization::Dedb;
    fast.segments.truncate(1);
    result.databases.push(fast);
    let ImsPcbMetadata::Database(mut pcb) = result.psbs[0].pcbs[0].clone() else {
        unreachable!()
    };
    pcb.name = "FASTPCB".into();
    pcb.database = "FASTDB".into();
    pcb.sensitive_segments.truncate(1);
    result.psbs[0].pcbs.push(ImsPcbMetadata::Database(pcb));
    result
}

fn exercise_system(store: Arc<dyn ProviderStateStore>) {
    let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    service.install_metadata(system_catalog()).unwrap();
    service
        .install_system_runtime(ImsSystemRuntimeDefinition {
            directory: Some(mainframe_env_host_api::ImsSystemDirectory {
                scd_address: 0x1000,
                pst_address: 0x2000,
            }),
            dedb_areas: vec![ImsDedbAreaDefinition {
                database: "FASTDB".into(),
                name: "FASTA".into(),
                sdep_capacity_cis: 20,
                iov_capacity_cis: 30,
            }],
            buffer_pools: vec![ImsBufferPoolDefinition {
                name: "OSAM1".into(),
                kind: ImsBufferPoolKind::Osam,
                buffer_bytes: 4096,
                buffers: 8,
            }],
        })
        .unwrap();
    let run = "system-run";
    let fast_run = "fast-run";
    assert_eq!(
        execute(
            &service,
            run,
            &request(run, ImsOperation::Schedule, 1, &[], b"")
        )
        .status,
        "  "
    );
    let mut schedule_fast = request(fast_run, ImsOperation::Schedule, 1, &[], b"");
    schedule_fast.pcb = 2;
    assert_eq!(execute(&service, fast_run, &schedule_fast).status, "  ");
    let query_run = "query-without-accept";
    assert_eq!(
        execute(
            &service,
            query_run,
            &request(query_run, ImsOperation::Schedule, 1, &[], b"")
        )
        .status,
        "  "
    );
    let direct_query = system_request(
        query_run,
        2,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Command,
        ImsSystemCall::Query { target_pcb: 1 },
    );
    assert!(matches!(execute(&service, query_run, &direct_query).system,
        Some(ImsSystemResult::Query { pcb }) if pcb.status == "  "));
    let accept = system_request(
        run,
        2,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Command,
        ImsSystemCall::Accept {
            row: ImsAcceptRow::Availability,
            group: ImsStatusGroup::A,
        },
    );
    assert_eq!(
        execute(&service, run, &accept).system,
        Some(ImsSystemResult::Accepted {
            group: ImsStatusGroup::A
        })
    );
    let query = system_request(
        run,
        3,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Command,
        ImsSystemCall::Query { target_pcb: 1 },
    );
    assert!(matches!(execute(&service, run, &query).system,
        Some(ImsSystemResult::Query { pcb }) if pcb.status == "  "));
    let provider_query = system_request(
        run,
        22,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Command,
        ImsSystemCall::Query { target_pcb: 2 },
    );
    let provider_invocation = invocation(run);
    let provider = ims_providers(service.clone(), InvocationLimits::default()).remove(1);
    let routed = provider.invoke(
        &provider_invocation,
        EffectRequest {
            run_unit: provider_invocation.run_unit_id.clone(),
            sequence: 22,
            deadline_tick: provider_invocation.deadline_tick,
            idempotency_key: provider_query
                .mutation
                .as_ref()
                .map(|mutation| mutation.idempotency_key.clone()),
            request: HostRequest::Ims(provider_query),
        },
    );
    assert!(matches!(
        routed.outcome,
        Ok(HostResult::Ims(ImsResult {
            system: Some(ImsSystemResult::Query { .. }),
            ..
        }))
    ));
    let refresh = system_request(
        run,
        4,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Command,
        ImsSystemCall::Refresh,
    );
    assert_eq!(
        service.execute(&invocation(run), &refresh),
        Err(HostProblem::Malformed)
    );
    assert_eq!(
        execute(
            &service,
            run,
            &request(run, ImsOperation::Insert, 5, &["ROOT"], b"A1X")
        )
        .status,
        "  "
    );
    assert!(matches!(execute(&service, run, &refresh).system,
        Some(ImsSystemResult::Refreshed { pcbs }) if pcbs.len() == 2));
    assert_eq!(
        service.execute(
            &invocation(run),
            &system_request(
                run,
                6,
                0,
                ImsExecutionContext::DbDc,
                ImsCallSyntax::Command,
                ImsSystemCall::Refresh
            )
        ),
        Err(HostProblem::Malformed)
    );
    assert_eq!(
        execute(
            &service,
            run,
            &request(run, ImsOperation::Insert, 7, &["ROOT"], b"B2Y")
        )
        .status,
        "  "
    );
    let mut first = request(run, ImsOperation::GetUnique, 8, &["ROOT"], b"");
    first.qualifiers = vec![qualifier(b"A1")];
    first.q_class = ImsQClass::new(b'A');
    assert_eq!(execute(&service, run, &first).status, "  ");
    let deq = system_request(
        run,
        9,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Dequeue {
            class: ImsQClass::new(b'A'),
        },
    );
    assert_eq!(
        execute(&service, run, &deq).system,
        Some(ImsSystemResult::Dequeued { released: 0 })
    );
    let mut second = request(run, ImsOperation::GetUnique, 10, &["ROOT"], b"");
    second.qualifiers = vec![qualifier(b"B2")];
    second.q_class = ImsQClass::new(b'A');
    assert_eq!(execute(&service, run, &second).status, "  ");
    let deq_release = system_request(
        run,
        11,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Dequeue {
            class: ImsQClass::new(b'A'),
        },
    );
    assert_eq!(
        execute(&service, run, &deq_release).system,
        Some(ImsSystemResult::Dequeued { released: 1 })
    );
    let mut missing = request(run, ImsOperation::GetUnique, 12, &["ROOT"], b"");
    missing.qualifiers = vec![qualifier(b"Z9")];
    assert_eq!(execute(&service, run, &missing).status, "GE");
    let stale_query = system_request(
        run,
        20,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Command,
        ImsSystemCall::Query { target_pcb: 1 },
    );
    assert!(matches!(execute(&service, run, &stale_query).system,
        Some(ImsSystemResult::Query { pcb }) if pcb.status == "  "));
    let late_refresh = system_request(
        run,
        21,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Command,
        ImsSystemCall::Refresh,
    );
    assert_eq!(
        service.execute(&invocation(run), &late_refresh),
        Err(HostProblem::Malformed)
    );
    let gscd = system_request(
        run,
        13,
        1,
        ImsExecutionContext::DbBatch,
        ImsCallSyntax::Call,
        ImsSystemCall::Gscd,
    );
    let applicable = system::resources(
        &service.lock().unwrap().state,
        &invocation_class(run, ServiceClass::Batch),
        &gscd,
    );
    assert!(applicable.is_ok(), "{applicable:?}");
    let gscd_result = service
        .execute(&invocation_class(run, ServiceClass::Batch), &gscd)
        .unwrap();
    assert_eq!(gscd_result.status, "GE");
    assert_eq!(
        gscd_result.system,
        Some(ImsSystemResult::Gscd {
            scd_address: 0x1000,
            pst_address: 0x2000,
        })
    );
    let gscd_io = system_request(
        run,
        17,
        0,
        ImsExecutionContext::DbBatch,
        ImsCallSyntax::Call,
        ImsSystemCall::Gscd,
    );
    assert_eq!(
        service
            .execute(&invocation_class(run, ServiceClass::Batch), &gscd_io)
            .unwrap()
            .status,
        "  "
    );
    service
        .publish_buffer_statistics(ImsBufferStatistics {
            pool: "OSAM1".into(),
            kind: ImsBufferPoolKind::Osam,
            buffer_bytes: 4096,
            buffers: 8,
            reads: 12,
            writes: 3,
        })
        .unwrap();
    let function = ImsStatisticsFunction {
        family: ImsStatisticsFamily::Dbas,
        format: ImsStatisticsFormat::Full,
        extended: false,
    };
    let stat = system_request(
        run,
        14,
        1,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Statistics { function },
    );
    let observed = execute(&service, run, &stat);
    assert!(
        matches!(observed.system, Some(ImsSystemResult::Statistics { pool: Some(ref pool) })
        if pool.reads == 12 && pool.writes == 3)
    );
    assert_eq!(execute(&service, run, &stat), observed);
    let exhausted = system_request(
        run,
        15,
        1,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Statistics { function },
    );
    assert_eq!(execute(&service, run, &exhausted).status, "GE");
    let no_vsam_pool = system_request(
        run,
        23,
        1,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Statistics {
            function: ImsStatisticsFunction {
                family: ImsStatisticsFamily::Vbas,
                format: ImsStatisticsFormat::Full,
                extended: false,
            },
        },
    );
    assert_eq!(execute(&service, run, &no_vsam_pool).status, "GE");
    let position = system_request(
        fast_run,
        2,
        2,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Position {
            ssa: None,
            keyword: ImsPositionKeyword::Default,
        },
    );
    assert!(matches!(execute(&service, fast_run, &position).system,
        Some(ImsSystemResult::Positioned { areas }) if areas[0].unused_iov_cis == 30));
    let initial = system_request(
        fast_run,
        5,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Accept {
            row: ImsAcceptRow::Initial,
            group: ImsStatusGroup::B,
        },
    );
    assert_eq!(
        execute(&service, fast_run, &initial).system,
        Some(ImsSystemResult::Accepted {
            group: ImsStatusGroup::B
        })
    );
    let dedb_deq = system_request(
        fast_run,
        6,
        2,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Dequeue { class: None },
    );
    assert_eq!(
        execute(&service, fast_run, &dedb_deq).system,
        Some(ImsSystemResult::Dequeued { released: 0 })
    );
    let qualified = system_request(
        fast_run,
        7,
        2,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Position {
            ssa: Some(ImsPositionSsa {
                segment: "ROOT".into(),
                field: Some("ROOTKEY".into()),
                value: Some(b"Z9".to_vec()),
            }),
            keyword: ImsPositionKeyword::Default,
        },
    );
    assert_eq!(execute(&service, fast_run, &qualified).status, "GE");
    service
        .publish_dedb_area_position(ImsPositionArea {
            name: "FASTA".into(),
            position: [0, 0, 0, 1, 0, 0, 0, 8],
            unused_sdep_cis: 19,
            unused_iov_cis: 29,
            timestamp: Some(123),
            ims_id: Some("IMSA".into()),
        })
        .unwrap();
    let before = service.lock().unwrap().state.clone();
    let forbidden = system_request(
        fast_run,
        3,
        2,
        ImsExecutionContext::DbBatch,
        ImsCallSyntax::Call,
        ImsSystemCall::Position {
            ssa: None,
            keyword: ImsPositionKeyword::Default,
        },
    );
    assert_eq!(
        service.execute(&invocation_class(fast_run, ServiceClass::Batch), &forbidden),
        Err(HostProblem::Unsupported)
    );
    let malformed = system_request(
        run,
        16,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Command,
        ImsSystemCall::Query { target_pcb: 0 },
    );
    assert_eq!(
        service.execute(&invocation(run), &malformed),
        Err(HostProblem::Malformed)
    );
    let malformed_stat = system_request(
        run,
        18,
        1,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Statistics {
            function: ImsStatisticsFunction {
                family: ImsStatisticsFamily::Vbas,
                format: ImsStatisticsFormat::Full,
                extended: true,
            },
        },
    );
    assert_eq!(
        service.execute(&invocation(run), &malformed_stat),
        Err(HostProblem::Malformed)
    );
    assert_eq!(service.lock().unwrap().state, before);
    drop(service);
    let reopened = ImsService::open(store, ImsLimits::default()).unwrap();
    assert_eq!(reopened.execute(&invocation(run), &stat).unwrap(), observed);
    let resumed = system_request(
        fast_run,
        4,
        2,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Position {
            ssa: None,
            keyword: ImsPositionKeyword::Default,
        },
    );
    assert!(matches!(execute(&reopened, fast_run, &resumed).system,
        Some(ImsSystemResult::Positioned { areas })
            if areas[0].position == [0, 0, 0, 1, 0, 0, 0, 8]));
    assert_eq!(
        system::reservation_count(&reopened.lock().unwrap().state),
        1
    );
}

#[test]
fn system_families_replay_and_restart_memory() {
    exercise_system(Arc::new(MemoryStore::new(Default::default())));
}

#[test]
fn system_families_replay_and_restart_sqlite() {
    let file = std::env::temp_dir().join(format!(
        "ims-system-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    let store: Arc<dyn ProviderStateStore> =
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    exercise_system(store);
    std::fs::remove_file(file).unwrap();
}

#[test]
fn system_authorization_precedes_stat_observation_and_replay() {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let policy = Arc::new(Policy::default());
    let service = ImsService::open_authorized(store, ImsLimits::default(), policy.clone()).unwrap();
    service.install_metadata(system_catalog()).unwrap();
    service
        .install_system_runtime(ImsSystemRuntimeDefinition {
            directory: None,
            dedb_areas: Vec::new(),
            buffer_pools: vec![ImsBufferPoolDefinition {
                name: "OSAM1".into(),
                kind: ImsBufferPoolKind::Osam,
                buffer_bytes: 4096,
                buffers: 8,
            }],
        })
        .unwrap();
    let run = "system-auth";
    execute(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    let stat = system_request(
        run,
        2,
        1,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Statistics {
            function: ImsStatisticsFunction {
                family: ImsStatisticsFamily::Dbas,
                format: ImsStatisticsFormat::Full,
                extended: false,
            },
        },
    );
    let before = service.lock().unwrap().state.clone();
    *policy.deny_read.lock().unwrap() = true;
    assert_eq!(
        service.execute(&invocation(run), &stat),
        Err(HostProblem::Unauthorized)
    );
    assert_eq!(service.lock().unwrap().state, before);
    *policy.deny_read.lock().unwrap() = false;
    assert!(matches!(
        execute(&service, run, &stat).system,
        Some(ImsSystemResult::Statistics { pool: Some(_) })
    ));
    *policy.deny_read.lock().unwrap() = true;
    assert_eq!(
        service.execute(&invocation(run), &stat),
        Err(HostProblem::Unauthorized)
    );
}
