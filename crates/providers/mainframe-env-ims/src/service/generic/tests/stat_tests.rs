//! Source-derived local regressions through the public host-provider route; zero official credit.
use super::*;
use mainframe_env_host_api::{
    EffectRequest, HostRequest, HostResult, ImsStatisticsObservationV2, ImsVsamSubpoolMetadata,
    ImsVsamSubpoolType,
};

mod failure_tests;

fn rows(store: &dyn ProviderStateStore) -> Vec<mainframe_env_store_api::ProviderStateRecord> {
    store.list_provider_state_prefix("ims-", 256).unwrap()
}

fn invoke(
    service: &Arc<ImsService>,
    run: &str,
    req: &ImsRequest,
) -> Result<ImsResult, HostProblem> {
    let invocation = invocation(run);
    invoke_context(service, &invocation, req)
}

fn invoke_context(
    service: &Arc<ImsService>,
    invocation: &Invocation,
    req: &ImsRequest,
) -> Result<ImsResult, HostProblem> {
    let providers = crate::ims_providers(service.clone(), InvocationLimits::default());
    let reply = providers[usize::from(req.operation.is_mutating())].invoke(
        invocation,
        EffectRequest {
            sequence: req.mutation.as_ref().map_or(99, |m| m.sequence),
            run_unit: invocation.run_unit_id.clone(),
            idempotency_key: req.mutation.as_ref().map(|m| m.idempotency_key.clone()),
            deadline_tick: invocation.deadline_tick,
            request: HostRequest::Ims(req.clone()),
        },
    );
    match reply.outcome? {
        HostResult::Ims(result) => Ok(result),
        other => panic!("unexpected reply: {other:?}"),
    }
}

fn setup() -> (Arc<MemoryStore>, Arc<ImsService>) {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let service = ImsService::open(store.clone(), Default::default()).unwrap();
    service.install_metadata(catalog()).unwrap();
    invoke(
        &service,
        "stat",
        &request("stat", ImsOperation::Schedule, 1, &[], b""),
    )
    .unwrap();
    (store, service)
}

fn stat_catalog() -> ImsMetadataCatalog {
    let mut catalog = system_catalog();
    let mut db = catalog.databases[0].clone();
    db.name = "OTHERDB".into();
    catalog.databases.push(db);
    let ImsPcbMetadata::Database(mut pcb) = catalog.psbs[0].pcbs[0].clone() else {
        unreachable!()
    };
    pcb.name = "OTHERPCB".into();
    pcb.database = "OTHERDB".into();
    catalog.psbs[0].pcbs.push(ImsPcbMetadata::Database(pcb));
    catalog
}

fn runtime_v2() -> ImsSystemRuntimeDefinition {
    let specs = [
        ("AINDEX", 1, 0, ImsVsamSubpoolType::Index, 512),
        ("ZDATA", 1, 0, ImsVsamSubpoolType::Data, 4096),
        ("MDATA", 1, 0, ImsVsamSubpoolType::Data, 1024),
        ("BFIRST", 0, 1, ImsVsamSubpoolType::Data, 256),
    ];
    ImsSystemRuntimeDefinition {
        buffer_pools: specs
            .iter()
            .map(|(name, _, _, _, size)| ImsBufferPoolDefinition {
                name: (*name).into(),
                kind: ImsBufferPoolKind::Vsam,
                buffer_bytes: *size,
                buffers: 2,
            })
            .chain([pool()])
            .collect(),
        vsam_subpools_v2: specs
            .iter()
            .map(|(name, id, order, kind, _)| ImsVsamSubpoolMetadata {
                subpool: (*name).into(),
                lsr_pool: *id,
                definition_order: *order,
                subpool_type: *kind,
            })
            .collect(),
        ..Default::default()
    }
}

fn install_v2(service: &Arc<ImsService>) {
    service.install_metadata(stat_catalog()).unwrap();
    let runtime = runtime_v2();
    service.install_system_runtime(runtime.clone()).unwrap();
    for p in &runtime.buffer_pools {
        service
            .publish_buffer_statistics(ImsBufferStatistics {
                pool: p.name.clone(),
                kind: p.kind,
                buffer_bytes: p.buffer_bytes,
                buffers: p.buffers,
                reads: u64::from(p.buffer_bytes),
                writes: 3,
            })
            .unwrap();
    }
    invoke(
        service,
        "stat",
        &request("stat", ImsOperation::Schedule, 1, &[], b""),
    )
    .unwrap();
}

fn v2(seq: u64, pcb: u16, family: ImsStatisticsFamily, format: ImsStatisticsFormat) -> ImsRequest {
    system_request(
        "stat",
        seq,
        pcb,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::StatisticsV2 {
            function: ImsStatisticsFunction {
                family,
                format,
                extended: false,
            },
            io_area_bytes: 360,
        },
    )
}

fn subpool(result: &ImsResult) -> &str {
    let Some(ImsSystemResult::StatisticsV2 {
        observation: Some(ImsStatisticsObservationV2::Subpool { statistics }),
        ..
    }) = &result.system
    else {
        panic!("expected subpool: {result:?}");
    };
    &statistics.pool
}

fn exercise_v2(store: Arc<dyn ProviderStateStore>) {
    let service = ImsService::open(store.clone(), Default::default()).unwrap();
    install_v2(&service);
    let formats = [
        ImsStatisticsFormat::Full,
        ImsStatisticsFormat::Summary,
        ImsStatisticsFormat::Unformatted,
    ];
    let first_request = v2(2, 1, ImsStatisticsFamily::Vbas, formats[0]);
    let first = invoke(&service, "stat", &first_request).unwrap();
    assert_eq!(subpool(&first), "MDATA");
    let before = rows(store.as_ref());
    assert_eq!(invoke(&service, "stat", &first_request), Ok(first.clone()));
    assert_eq!(rows(store.as_ref()), before);
    let mut conflicting = first_request.clone();
    if let Some(ImsSystemRequest {
        call: ImsSystemCall::StatisticsV2 { io_area_bytes, .. },
        ..
    }) = &mut conflicting.system
    {
        *io_area_bytes = 361;
    }
    assert_eq!(
        invoke(&service, "stat", &conflicting),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(rows(store.as_ref()), before);
    drop(service);
    let service = ImsService::open(store.clone(), Default::default()).unwrap();
    assert_eq!(invoke(&service, "stat", &first_request), Ok(first));
    assert_eq!(
        subpool(
            &invoke(
                &service,
                "stat",
                &v2(3, 3, ImsStatisticsFamily::Vbas, formats[1])
            )
            .unwrap()
        ),
        "MDATA"
    );
    for (seq, name, format) in [
        (4, "ZDATA", formats[1]),
        (5, "AINDEX", formats[2]),
        (6, "BFIRST", formats[0]),
    ] {
        let r = invoke(
            &service,
            "stat",
            &v2(seq, 1, ImsStatisticsFamily::Vbas, format),
        )
        .unwrap();
        assert_eq!(subpool(&r), name);
        let Some(ImsSystemResult::StatisticsV2 { function, .. }) = r.system else {
            unreachable!()
        };
        assert_eq!(function.format, format);
    }
    let total = invoke(
        &service,
        "stat",
        &v2(7, 1, ImsStatisticsFamily::Vbas, formats[0]),
    )
    .unwrap();
    assert_eq!(total.status, "GA");
    assert!(matches!(
        total.system,
        Some(ImsSystemResult::StatisticsV2 {
            observation: Some(ImsStatisticsObservationV2::Totals {
                buffers: 8,
                storage_bytes: 11776,
                reads: 5888,
                writes: 12
            }),
            ..
        })
    ));
    assert_eq!(
        invoke(
            &service,
            "stat",
            &v2(8, 1, ImsStatisticsFamily::Vbas, formats[0])
        ),
        Ok(total)
    );
    // A not-found DB call still uses PCB 1 and resets only that PCB's series.
    let mut get = request("stat", ImsOperation::GetUnique, 9, &["ROOT"], b"");
    get.mutation = Some(Mutation {
        sequence: 9,
        idempotency_key: IdempotencyKey::new("stat-9", InvocationLimits::default()).unwrap(),
        transaction: None,
    });
    assert_eq!(invoke(&service, "stat", &get).unwrap().status, "GE");
    assert_eq!(
        subpool(
            &invoke(
                &service,
                "stat",
                &v2(10, 1, ImsStatisticsFamily::Vbas, formats[0])
            )
            .unwrap()
        ),
        "MDATA"
    );
    assert_eq!(
        subpool(
            &invoke(
                &service,
                "stat",
                &v2(11, 3, ImsStatisticsFamily::Vbas, formats[0])
            )
            .unwrap()
        ),
        "ZDATA"
    );
    for (i, format) in formats.into_iter().enumerate() {
        let r = invoke(
            &service,
            "stat",
            &v2(12 + i as u64, 1, ImsStatisticsFamily::Dbas, format),
        )
        .unwrap();
        assert_eq!(r.status, "  ");
        assert!(
            matches!(r.system, Some(ImsSystemResult::StatisticsV2 { function,
            observation: Some(ImsStatisticsObservationV2::Totals { buffers: 8, storage_bytes: 32768, reads: 4096, writes: 3 })
        }) if function.format == format)
        );
    }
}

#[test]
fn stat_v2_public_memory_order_totals_formats_selected_pcb_replay_and_reset() {
    exercise_v2(Arc::new(MemoryStore::new(Default::default())));
}

#[test]
fn stat_v2_public_sqlite_order_totals_formats_selected_pcb_replay_and_reset() {
    let file = std::env::temp_dir().join(format!(
        "ims-stat-v2-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    exercise_v2(Arc::new(
        SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).unwrap(),
    ));
    // Fresh adapter, not merely a provider on the previous connection.
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).unwrap());
    let service = ImsService::open(store.clone(), Default::default()).unwrap();
    assert_eq!(
        subpool(
            &invoke(
                &service,
                "stat",
                &v2(15, 3, ImsStatisticsFamily::Vbas, ImsStatisticsFormat::Full)
            )
            .unwrap()
        ),
        "AINDEX"
    );
    let replay = v2(2, 1, ImsStatisticsFamily::Vbas, ImsStatisticsFormat::Full);
    let before = rows(store.as_ref());
    assert_eq!(
        subpool(&invoke(&service, "stat", &replay).unwrap()),
        "MDATA"
    );
    assert_eq!(rows(store.as_ref()), before);
    drop(service);
    drop(store);
    std::fs::remove_file(file).unwrap();
}

#[test]
fn stat_v2_capacity_context_and_unsupported_forms_do_not_mutate() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let service = ImsService::open(store.clone(), Default::default()).unwrap();
    install_v2(&service);
    let before = rows(store.as_ref());
    let mut seq = 100;
    for family in [
        ImsStatisticsFamily::Dbas,
        ImsStatisticsFamily::Dbes,
        ImsStatisticsFamily::Vbas,
        ImsStatisticsFamily::Vbes,
    ] {
        for format in [
            ImsStatisticsFormat::Full,
            ImsStatisticsFormat::Osam,
            ImsStatisticsFormat::Summary,
            ImsStatisticsFormat::Unformatted,
        ] {
            for extended in [false, true] {
                let function = ImsStatisticsFunction {
                    family,
                    format,
                    extended,
                };
                let mut req = v2(seq, 1, family, format);
                seq += 1;
                req.system.as_mut().unwrap().call = ImsSystemCall::StatisticsV2 {
                    function,
                    io_area_bytes: 600,
                };
                let malformed = extended
                    && !(family == ImsStatisticsFamily::Dbes
                        && format != ImsStatisticsFormat::Summary)
                    || format == ImsStatisticsFormat::Osam
                        && matches!(
                            family,
                            ImsStatisticsFamily::Vbas | ImsStatisticsFamily::Vbes
                        );
                let expected = if malformed {
                    Some(HostProblem::Malformed)
                } else if extended
                    || format == ImsStatisticsFormat::Osam
                    || matches!(
                        family,
                        ImsStatisticsFamily::Dbes | ImsStatisticsFamily::Vbes
                    )
                {
                    Some(HostProblem::Unsupported)
                } else {
                    None
                };
                if let Some(error) = expected {
                    assert_eq!(invoke(&service, "stat", &req), Err(error), "{function:?}");
                    assert_eq!(rows(store.as_ref()), before);
                }
            }
        }
    }
    for (format, minimum) in [
        (ImsStatisticsFormat::Full, 360),
        (ImsStatisticsFormat::Summary, 180),
        (ImsStatisticsFormat::Unformatted, 72),
    ] {
        for family in [ImsStatisticsFamily::Dbas, ImsStatisticsFamily::Vbas] {
            let mut req = v2(seq, 1, family, format);
            seq += 1;
            let ImsSystemCall::StatisticsV2 { io_area_bytes, .. } =
                &mut req.system.as_mut().unwrap().call
            else {
                unreachable!()
            };
            *io_area_bytes = minimum - 1;
            assert_eq!(invoke(&service, "stat", &req), Err(HostProblem::Malformed));
            assert_eq!(rows(store.as_ref()), before);
        }
    }
    for context in [
        ImsExecutionContext::Dcctl,
        ImsExecutionContext::TmBatch,
        ImsExecutionContext::DbBatch,
    ] {
        let mut req = v2(seq, 1, ImsStatisticsFamily::Dbas, ImsStatisticsFormat::Full);
        seq += 1;
        req.system.as_mut().unwrap().context = context;
        assert_eq!(
            invoke(&service, "stat", &req),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(rows(store.as_ref()), before);
    }
    for (pcb, error) in [
        (0, HostProblem::Malformed),
        (2, HostProblem::Unsupported),
        (4, HostProblem::NotFound),
    ] {
        assert_eq!(
            invoke(
                &service,
                "stat",
                &v2(
                    seq,
                    pcb,
                    ImsStatisticsFamily::Dbas,
                    ImsStatisticsFormat::Full
                )
            ),
            Err(error)
        );
        seq += 1;
        assert_eq!(rows(store.as_ref()), before);
    }
    for (i, context) in [ImsExecutionContext::DbDc, ImsExecutionContext::Dbctl]
        .into_iter()
        .enumerate()
    {
        let mut req = v2(
            seq + 1 + i as u64,
            1,
            ImsStatisticsFamily::Dbas,
            ImsStatisticsFormat::Summary,
        );
        req.system.as_mut().unwrap().context = context;
        if let ImsSystemCall::StatisticsV2 { io_area_bytes, .. } =
            &mut req.system.as_mut().unwrap().call
        {
            *io_area_bytes = 180;
        }
        req.system.as_mut().unwrap().syntax = ImsCallSyntax::Command;
        assert_eq!(invoke(&service, "stat", &req).unwrap().status, "  ");
    }
    let mut req = v2(
        seq + 4,
        1,
        ImsStatisticsFamily::Dbas,
        ImsStatisticsFormat::Unformatted,
    );
    req.system.as_mut().unwrap().context = ImsExecutionContext::DbBatch;
    if let ImsSystemCall::StatisticsV2 { io_area_bytes, .. } =
        &mut req.system.as_mut().unwrap().call
    {
        *io_area_bytes = 72;
    }
    assert_eq!(
        invoke_context(
            &service,
            &invocation_class("stat", ServiceClass::Batch),
            &req
        )
        .unwrap()
        .status,
        "  "
    );
}

fn pool() -> ImsBufferPoolDefinition {
    ImsBufferPoolDefinition {
        name: "OSAM".into(),
        kind: ImsBufferPoolKind::Osam,
        buffer_bytes: 4096,
        buffers: 8,
    }
}

fn stat(sequence: u64, family: ImsStatisticsFamily, extended: bool) -> ImsRequest {
    system_request(
        "stat",
        sequence,
        1,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Statistics {
            function: ImsStatisticsFunction {
                family,
                format: ImsStatisticsFormat::Full,
                extended,
            },
        },
    )
}

fn publish(service: &ImsService) {
    service
        .publish_buffer_statistics(ImsBufferStatistics {
            pool: "OSAM".into(),
            kind: ImsBufferPoolKind::Osam,
            buffer_bytes: 4096,
            buffers: 8,
            reads: 12,
            writes: 3,
        })
        .unwrap();
}

#[test]
fn stat_basic_osam_is_not_a_subpool_iterator() {
    let (_, service) = setup();
    service
        .install_system_runtime(ImsSystemRuntimeDefinition {
            buffer_pools: vec![pool()],
            ..Default::default()
        })
        .unwrap();
    publish(&service);
    let first = invoke(&service, "stat", &stat(2, ImsStatisticsFamily::Dbas, false)).unwrap();
    let next = invoke(&service, "stat", &stat(3, ImsStatisticsFamily::Dbas, false)).unwrap();
    assert_eq!(first.status, "  ");
    assert_eq!(next, first, "DBAS reports the pool on every call");
}

#[test]
fn stat_unavailable_counters_do_not_claim_zero_measurements() {
    let (store, service) = setup();
    service
        .install_system_runtime(ImsSystemRuntimeDefinition {
            buffer_pools: vec![pool()],
            ..Default::default()
        })
        .unwrap();
    let before = store.list_provider_state_prefix("ims-", 128).unwrap();
    assert_eq!(
        invoke(&service, "stat", &stat(2, ImsStatisticsFamily::Dbas, false)),
        Err(HostProblem::NotFound)
    );
    assert_eq!(
        store.list_provider_state_prefix("ims-", 128).unwrap(),
        before
    );
}

#[test]
fn stat_extended_requires_real_extended_counters() {
    let (store, service) = setup();
    service
        .install_system_runtime(ImsSystemRuntimeDefinition {
            buffer_pools: vec![pool()],
            ..Default::default()
        })
        .unwrap();
    publish(&service);
    let before = store.list_provider_state_prefix("ims-", 128).unwrap();
    assert_eq!(
        invoke(&service, "stat", &stat(2, ImsStatisticsFamily::Dbes, true)),
        Err(HostProblem::Unsupported)
    );
    assert_eq!(
        store.list_provider_state_prefix("ims-", 128).unwrap(),
        before
    );
}
